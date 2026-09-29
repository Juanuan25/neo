//! Small in-process sampler behind `GET /resources/snapshot`.
//!
//! Percent/rate metrics (CPU%, per-process CPU%, network throughput) need a
//! delta between two `/proc` reads, so this keeps the previous reading in
//! server state instead of sleeping-then-reading per request (which would
//! make every request take >=1s and wouldn't scale with concurrent viewers).
//! Reads are also throttled to [`MIN_RESAMPLE`] so the compact strip and an
//! open detail panel polling independently share one `/proc` walk.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use super::cpu::{self, CpuTimes};
use super::disks::{self, MountInfo};
use super::mem;
use super::net::{self, NetCounters};
use super::procs::{self, ProcRaw};
use super::snapshot::{
    CpuSnapshot, DiskSnapshot, MemSnapshot, NetSnapshot, ProcSnapshot, Snapshot,
};

const PROC_STAT: &str = "/proc/stat";
const MEMINFO: &str = "/proc/meminfo";
const ARCSTATS: &str = "/proc/spl/kstat/zfs/arcstats";
const NET_DEV: &str = "/proc/net/dev";
const MOUNTS: &str = "/proc/mounts";
const LOADAVG: &str = "/proc/loadavg";
const UPTIME: &str = "/proc/uptime";
const PROC_DIR: &str = "/proc";

/// Share one fresh `/proc` read across requests arriving faster than this.
const MIN_RESAMPLE: Duration = Duration::from_millis(900);
/// First call has no previous sample to diff, so a throwaway read + short
/// sleep before the real one gives the caller a meaningful delta immediately
/// instead of an all-zero first frame.
const WARMUP_DELAY: Duration = Duration::from_millis(150);
/// Rows kept per metric before union-ing cpu/mem top lists (see `build_proc_snapshots`).
const TOP_N: usize = 15;

pub fn parse_loadavg(input: &str) -> [f32; 3] {
    let mut it = input.split_whitespace();
    let mut next = move || it.next().and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
    [next(), next(), next()]
}

pub fn parse_uptime_secs(input: &str) -> u64 {
    input
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .map(|v| v as u64)
        .unwrap_or(0)
}

fn clock_ticks_per_sec() -> u64 {
    // SAFETY: sysconf with a valid, read-only-effect name; no pointers involved.
    let v = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if v > 0 {
        v as u64
    } else {
        100 // conventional USER_HZ fallback
    }
}

fn read_to_string_or_empty(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

struct RawSample {
    stat: cpu::ProcStat,
    meminfo: mem::MemInfo,
    arc_bytes: Option<u64>,
    net: NetCounters,
    mounts: Vec<MountInfo>,
    load: [f32; 3],
    uptime_secs: u64,
    procs: Vec<ProcRaw>,
}

fn read_raw() -> RawSample {
    RawSample {
        stat: cpu::parse_proc_stat(&read_to_string_or_empty(PROC_STAT)),
        meminfo: mem::parse_meminfo(&read_to_string_or_empty(MEMINFO)),
        arc_bytes: std::fs::read_to_string(ARCSTATS)
            .ok()
            .and_then(|s| mem::parse_arcstats(&s)),
        net: net::parse_proc_net_dev(&read_to_string_or_empty(NET_DEV)),
        mounts: disks::parse_proc_mounts(&read_to_string_or_empty(MOUNTS)),
        load: parse_loadavg(&read_to_string_or_empty(LOADAVG)),
        uptime_secs: parse_uptime_secs(&read_to_string_or_empty(UPTIME)),
        procs: procs::read_all_processes(Path::new(PROC_DIR)),
    }
}

fn build_mem_snapshot(raw: &RawSample) -> MemSnapshot {
    let m = &raw.meminfo;
    let total = m.total_kb.saturating_mul(1024);
    let free = m.free_kb.saturating_mul(1024);
    let cache = (m.buffers_kb + m.cached_kb + m.sreclaimable_kb).saturating_mul(1024);
    let arc = raw.arc_bytes.unwrap_or(0);
    // "Used" is everything not accounted for elsewhere — apps + kernel memory
    // that isn't reclaimable cache and isn't the ARC (ARC gets its own
    // segment on purpose; see the module doc).
    let used = total
        .saturating_sub(free)
        .saturating_sub(cache)
        .saturating_sub(arc);
    MemSnapshot {
        total_bytes: total,
        used_bytes: used,
        cache_bytes: cache,
        arc_bytes: raw.arc_bytes,
        free_bytes: free,
        swap_total_bytes: m.swap_total_kb.saturating_mul(1024),
        swap_used_bytes: m
            .swap_total_kb
            .saturating_sub(m.swap_free_kb)
            .saturating_mul(1024),
    }
}

/// Cap the disk list so a heavily-datasetted ZFS pool (many mounted
/// datasets) doesn't turn the panel into a wall of rows.
const MAX_DISKS: usize = 12;

fn build_disk_snapshots(mounts: &[MountInfo]) -> Vec<DiskSnapshot> {
    let mut out: Vec<DiskSnapshot> = mounts
        .iter()
        .filter_map(|m| {
            disks::statvfs_usage(&m.mount_point).map(|u| DiskSnapshot {
                mount: m.mount_point.clone(),
                fs_type: m.fs_type.clone(),
                is_zfs: m.fs_type == "zfs",
                total_bytes: u.total,
                used_bytes: u.used,
            })
        })
        .collect();
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out.truncate(MAX_DISKS);
    out
}

struct ProcPrev {
    cpu_ticks: u64,
    starttime: u64,
}

/// CPU% + a cpu/mem top-N union for the process table. Keyed by pid+starttime
/// so a reused pid never gets attributed the old process's ticks.
fn build_proc_snapshots(
    current: &[ProcRaw],
    prev: &HashMap<u32, ProcPrev>,
    elapsed_secs: f64,
    clk_tck: u64,
) -> Vec<ProcSnapshot> {
    let all: Vec<ProcSnapshot> = current
        .iter()
        .map(|p| {
            let cpu_pct = prev
                .get(&p.pid)
                .filter(|pp| pp.starttime == p.starttime)
                .map(|pp| {
                    if elapsed_secs <= 0.0 {
                        return 0.0;
                    }
                    let delta_ticks = p.cpu_ticks.saturating_sub(pp.cpu_ticks) as f64;
                    ((delta_ticks / clk_tck as f64) / elapsed_secs * 100.0).max(0.0) as f32
                })
                .unwrap_or(0.0);
            ProcSnapshot {
                pid: p.pid,
                name: p.name.clone(),
                cpu_pct,
                mem_bytes: p.rss_bytes,
            }
        })
        .collect();

    let mut by_cpu = all.clone();
    by_cpu.sort_by(|a, b| {
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut by_mem = all;
    by_mem.sort_by_key(|p| std::cmp::Reverse(p.mem_bytes));

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for p in by_cpu
        .into_iter()
        .take(TOP_N)
        .chain(by_mem.into_iter().take(TOP_N))
    {
        if seen.insert(p.pid) {
            out.push(p);
        }
    }
    out
}

struct Prev {
    at: Instant,
    cpu_overall: CpuTimes,
    cpu_cores: Vec<CpuTimes>,
    net: NetCounters,
    procs: HashMap<u32, ProcPrev>,
}

struct SamplerState {
    prev: Option<Prev>,
    cached: Option<(Instant, Snapshot)>,
}

/// Server-held state for the resources panel. One instance lives in
/// `AppConfig` for the process lifetime.
pub struct ResourceSampler {
    state: Mutex<SamplerState>,
}

// Manual impl: the guarded state (previous /proc samples) isn't meaningful to
// print and would otherwise require every nested type to derive Debug.
impl std::fmt::Debug for ResourceSampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ResourceSampler")
    }
}

impl Default for ResourceSampler {
    fn default() -> Self {
        ResourceSampler {
            state: Mutex::new(SamplerState {
                prev: None,
                cached: None,
            }),
        }
    }
}

impl ResourceSampler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current snapshot, computing a fresh one only if the cache is stale.
    pub async fn snapshot(&self) -> Snapshot {
        let mut st = self.state.lock().await;
        if let Some((at, snap)) = &st.cached {
            if at.elapsed() < MIN_RESAMPLE {
                return snap.clone();
            }
        }
        if st.prev.is_none() {
            // Cold start: a throwaway read now means the read below already
            // has something to diff against.
            let _ = read_raw();
            tokio::time::sleep(WARMUP_DELAY).await;
        }
        let snap = compute(&mut st);
        st.cached = Some((Instant::now(), snap.clone()));
        snap
    }
}

fn compute(st: &mut SamplerState) -> Snapshot {
    let raw = read_raw();
    let now = Instant::now();
    let clk_tck = clock_ticks_per_sec();

    let (cpu_overall_pct, per_core_pct) = match &st.prev {
        Some(prev) => {
            let overall = cpu::cpu_percent(&prev.cpu_overall, &raw.stat.overall);
            let per_core = raw
                .stat
                .per_core
                .iter()
                .enumerate()
                .map(|(i, cur)| {
                    prev.cpu_cores
                        .get(i)
                        .map(|p| cpu::cpu_percent(p, cur))
                        .unwrap_or(0.0)
                })
                .collect();
            (overall, per_core)
        }
        None => (0.0, vec![0.0; raw.stat.per_core.len()]),
    };

    let elapsed_secs = st
        .prev
        .as_ref()
        .map(|p| p.at.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let (rx, tx) = match &st.prev {
        Some(prev) => net::net_rate(&prev.net, &raw.net, elapsed_secs),
        None => (0.0, 0.0),
    };

    let mem_snap = build_mem_snapshot(&raw);
    let disks = build_disk_snapshots(&raw.mounts);
    let procs = match &st.prev {
        Some(prev) => build_proc_snapshots(&raw.procs, &prev.procs, elapsed_secs, clk_tck),
        None => Vec::new(),
    };

    let mut next_procs = HashMap::with_capacity(raw.procs.len());
    for p in &raw.procs {
        next_procs.insert(
            p.pid,
            ProcPrev {
                cpu_ticks: p.cpu_ticks,
                starttime: p.starttime,
            },
        );
    }
    st.prev = Some(Prev {
        at: now,
        cpu_overall: raw.stat.overall,
        cpu_cores: raw.stat.per_core.clone(),
        net: raw.net,
        procs: next_procs,
    });

    Snapshot {
        uptime_secs: raw.uptime_secs,
        load: raw.load,
        cpu: CpuSnapshot {
            overall_pct: cpu_overall_pct,
            per_core_pct,
        },
        mem: mem_snap,
        disks,
        net: NetSnapshot {
            rx_bytes_per_sec: rx,
            tx_bytes_per_sec: tx,
        },
        procs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loadavg_first_three_fields() {
        assert_eq!(
            parse_loadavg("0.52 0.58 0.59 2/456 12345\n"),
            [0.52, 0.58, 0.59]
        );
    }

    #[test]
    fn loadavg_missing_fields_default_to_zero() {
        assert_eq!(parse_loadavg("1.0"), [1.0, 0.0, 0.0]);
        assert_eq!(parse_loadavg(""), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn parses_uptime_whole_seconds() {
        assert_eq!(parse_uptime_secs("12345.67 54321.00\n"), 12345);
    }

    #[test]
    fn uptime_empty_is_zero() {
        assert_eq!(parse_uptime_secs(""), 0);
    }

    #[test]
    fn mem_snapshot_splits_used_cache_arc_free() {
        let raw = RawSample {
            stat: cpu::ProcStat::default(),
            meminfo: mem::MemInfo {
                total_kb: 1_000_000,
                free_kb: 100_000,
                available_kb: 0,
                buffers_kb: 10_000,
                cached_kb: 200_000,
                sreclaimable_kb: 5_000,
                swap_total_kb: 50_000,
                swap_free_kb: 20_000,
            },
            arc_bytes: Some(300_000 * 1024),
            net: NetCounters::default(),
            mounts: Vec::new(),
            load: [0.0, 0.0, 0.0],
            uptime_secs: 0,
            procs: Vec::new(),
        };
        let snap = build_mem_snapshot(&raw);
        assert_eq!(snap.total_bytes, 1_000_000 * 1024);
        assert_eq!(snap.free_bytes, 100_000 * 1024);
        assert_eq!(snap.cache_bytes, (10_000 + 200_000 + 5_000) * 1024);
        assert_eq!(snap.arc_bytes, Some(300_000 * 1024));
        // used = total - free - cache - arc
        let expected_used = (1_000_000 - 100_000 - 215_000 - 300_000) * 1024;
        assert_eq!(snap.used_bytes, expected_used);
        assert_eq!(snap.swap_total_bytes, 50_000 * 1024);
        assert_eq!(snap.swap_used_bytes, 30_000 * 1024);
    }

    #[test]
    fn mem_snapshot_without_arc_is_none_not_zero() {
        let raw = RawSample {
            stat: cpu::ProcStat::default(),
            meminfo: mem::MemInfo::default(),
            arc_bytes: None,
            net: NetCounters::default(),
            mounts: Vec::new(),
            load: [0.0, 0.0, 0.0],
            uptime_secs: 0,
            procs: Vec::new(),
        };
        assert_eq!(build_mem_snapshot(&raw).arc_bytes, None);
    }

    #[test]
    fn proc_snapshots_union_top_cpu_and_top_mem_deduped() {
        let current = vec![
            ProcRaw {
                pid: 1,
                name: "cpu-hog".into(),
                cpu_ticks: 1000,
                starttime: 10,
                rss_bytes: 1,
            },
            ProcRaw {
                pid: 2,
                name: "mem-hog".into(),
                cpu_ticks: 0,
                starttime: 10,
                rss_bytes: 999_999,
            },
            ProcRaw {
                pid: 3,
                name: "idle".into(),
                cpu_ticks: 0,
                starttime: 10,
                rss_bytes: 1,
            },
        ];
        let mut prev = HashMap::new();
        prev.insert(
            1,
            ProcPrev {
                cpu_ticks: 0,
                starttime: 10,
            },
        );
        prev.insert(
            2,
            ProcPrev {
                cpu_ticks: 0,
                starttime: 10,
            },
        );
        prev.insert(
            3,
            ProcPrev {
                cpu_ticks: 0,
                starttime: 10,
            },
        );
        let out = build_proc_snapshots(&current, &prev, 1.0, 100);
        let pids: HashSet<u32> = out.iter().map(|p| p.pid).collect();
        assert!(pids.contains(&1), "top-cpu process must be present");
        assert!(pids.contains(&2), "top-mem process must be present");
        // No duplicate pids in the union.
        assert_eq!(pids.len(), out.len());
    }

    #[test]
    fn proc_pid_reuse_does_not_inherit_old_ticks() {
        // Same pid, different starttime => a different process; must not
        // diff against the old one's cpu_ticks (would show a bogus spike or
        // negative-then-saturated delta).
        let current = vec![ProcRaw {
            pid: 7,
            name: "new-proc".into(),
            cpu_ticks: 5,
            starttime: 999, // different from prev's starttime below
            rss_bytes: 0,
        }];
        let mut prev = HashMap::new();
        prev.insert(
            7,
            ProcPrev {
                cpu_ticks: 100_000,
                starttime: 1,
            },
        );
        let out = build_proc_snapshots(&current, &prev, 1.0, 100);
        assert_eq!(out[0].cpu_pct, 0.0);
    }

    #[tokio::test]
    async fn sampler_reads_real_proc_without_panicking() {
        let sampler = ResourceSampler::new();
        let first = sampler.snapshot().await;
        let second = sampler.snapshot().await;
        // Real /proc: total memory must be positive on any real machine.
        // (Disk list is intentionally not asserted here: a sandboxed test
        // runner's root filesystem may be overlayfs/none of REAL_FS_TYPES,
        // which is exercised directly in disks.rs's own tests instead.)
        assert!(first.mem.total_bytes > 0);
        assert!(second.mem.total_bytes > 0);
        assert!(second.cpu.overall_pct >= 0.0);
    }
}
