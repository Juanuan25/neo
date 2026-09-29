//! Per-process sampling: `/proc/<pid>/stat` (cpu ticks), `/status` (RSS),
//! `/cmdline` (display name). CPU% needs a delta between two samples of the
//! same process (keyed by pid *and* starttime, since pids get reused), which
//! the sampler owns; this module only does parsing + the one-shot `/proc`
//! walk.

use std::fs;
use std::path::Path;

/// Fields pulled from `/proc/<pid>/stat` needed for CPU% and display.
#[derive(Clone, Debug, PartialEq)]
pub struct PidStat {
    pub pid: u32,
    /// Command name from the parenthesized field (kernel-truncated to 15 chars).
    pub comm: String,
    /// Ticks (user+sys) attributed to this process since boot.
    pub cpu_ticks: u64,
    /// Start time in ticks since boot — combined with `pid`, uniquely
    /// identifies a process across pid reuse.
    pub starttime: u64,
}

/// Parse one `/proc/<pid>/stat` file's contents. The `comm` field is
/// parenthesized and may itself contain spaces or parentheses (process names
/// are attacker/user controlled), so this looks for the *last* `)` rather
/// than splitting naively on whitespace.
pub fn parse_pid_stat(input: &str) -> Option<PidStat> {
    let line = input.lines().next()?;
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close < open {
        return None;
    }
    let pid = line[..open].trim().parse::<u32>().ok()?;
    let comm = line[open + 1..close].to_string();
    let rest = line[close + 1..].trim_start();
    let fields: Vec<&str> = rest.split_whitespace().collect();
    // field indices below are 0-based into `fields`, which starts at stat
    // field 3 (state) — see the `proc(5)` man page's `stat` table.
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let starttime: u64 = fields.get(19)?.parse().ok()?;
    Some(PidStat {
        pid,
        comm,
        cpu_ticks: utime.saturating_add(stime),
        starttime,
    })
}

/// Parse `VmRSS` (kB) out of `/proc/<pid>/status`; `None` if absent (process
/// exited between listing and reading, or a kernel without the field).
pub fn parse_status_vmrss_kb(input: &str) -> Option<u64> {
    for line in input.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse::<u64>().ok();
        }
    }
    None
}

/// Build a display name from `/proc/<pid>/cmdline` bytes (NUL-separated
/// argv), falling back to `[comm]` for kernel threads / zombies with no
/// cmdline (matches `ps`'s bracket convention).
pub fn display_name(cmdline_raw: &str, comm: &str) -> String {
    let joined = cmdline_raw
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        format!("[{comm}]")
    } else {
        joined
    }
}

/// One process, freshly read from `/proc` (cpu%/mem not yet computed —
/// that's the sampler's job once it has a previous sample to diff against).
#[derive(Clone, Debug, PartialEq)]
pub struct ProcRaw {
    pub pid: u32,
    pub name: String,
    pub cpu_ticks: u64,
    pub starttime: u64,
    pub rss_bytes: u64,
}

/// List every readable process under `proc_dir` (normally `/proc`). Processes
/// that vanish mid-read (exited between `read_dir` and the per-pid reads) are
/// silently skipped — this always returns "whatever was readable just now",
/// never an error, matching the rest of the status code's "never fails the
/// page" convention.
pub fn read_all_processes(proc_dir: &Path) -> Vec<ProcRaw> {
    let Ok(entries) = fs::read_dir(proc_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.parse::<u32>().is_err() {
            continue;
        }
        let pid_dir = entry.path();
        let Ok(stat_raw) = fs::read_to_string(pid_dir.join("stat")) else {
            continue;
        };
        let Some(stat) = parse_pid_stat(&stat_raw) else {
            continue;
        };
        let rss_kb = fs::read_to_string(pid_dir.join("status"))
            .ok()
            .and_then(|s| parse_status_vmrss_kb(&s))
            .unwrap_or(0);
        let cmdline_raw = fs::read_to_string(pid_dir.join("cmdline")).unwrap_or_default();
        out.push(ProcRaw {
            pid: stat.pid,
            name: display_name(&cmdline_raw, &stat.comm),
            cpu_ticks: stat.cpu_ticks,
            starttime: stat.starttime,
            rss_bytes: rss_kb.saturating_mul(1024),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordinary_process() {
        // Truncated but field-count-correct /proc/<pid>/stat line.
        let line = "1234 (nginx) S 1 1234 1234 0 -1 4194304 100 0 0 0 55 10 0 0 20 0 2 0 987654 123456 4096 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 3 0 0 0 0 0";
        let s = parse_pid_stat(line).expect("parses");
        assert_eq!(s.pid, 1234);
        assert_eq!(s.comm, "nginx");
        assert_eq!(s.cpu_ticks, 55 + 10);
        assert_eq!(s.starttime, 987654);
    }

    #[test]
    fn comm_with_spaces_and_parens_is_handled_via_last_paren() {
        // e.g. a process literally named "weird (proc) name" by execve argv[0].
        let line = "42 (weird (proc) name) S 1 42 42 0 -1 4194304 0 0 0 0 7 3 0 0 20 0 1 0 100 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1";
        let s = parse_pid_stat(line).expect("parses");
        assert_eq!(s.pid, 42);
        assert_eq!(s.comm, "weird (proc) name");
        assert_eq!(s.cpu_ticks, 10);
    }

    #[test]
    fn malformed_stat_is_none_not_panic() {
        assert_eq!(parse_pid_stat(""), None);
        assert_eq!(parse_pid_stat("not a stat line"), None);
        assert_eq!(parse_pid_stat("abc (comm) S"), None); // pid not numeric
    }

    #[test]
    fn parses_vmrss_from_status() {
        let status = "Name:\tnginx\nVmPeak:\t  12345 kB\nVmRSS:\t   4321 kB\nThreads:\t1\n";
        assert_eq!(parse_status_vmrss_kb(status), Some(4321));
    }

    #[test]
    fn missing_vmrss_is_none() {
        assert_eq!(parse_status_vmrss_kb("Name:\tfoo\n"), None);
    }

    #[test]
    fn display_name_uses_cmdline_when_present() {
        let raw = "docker\0run\0-it\0alpine\0";
        assert_eq!(display_name(raw, "docker"), "docker run -it alpine");
    }

    #[test]
    fn display_name_falls_back_to_bracketed_comm_for_kernel_threads() {
        assert_eq!(display_name("", "kswapd0"), "[kswapd0]");
    }

    #[test]
    fn read_all_processes_on_missing_dir_is_empty_not_error() {
        assert!(read_all_processes(Path::new("/does/not/exist/neo-test")).is_empty());
    }

    #[test]
    fn read_all_processes_on_real_proc_finds_self() {
        let pid = std::process::id();
        let procs = read_all_processes(Path::new("/proc"));
        assert!(
            procs.iter().any(|p| p.pid == pid),
            "expected to find our own pid {pid} in /proc listing"
        );
    }
}
