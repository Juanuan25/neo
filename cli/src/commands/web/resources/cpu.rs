//! `/proc/stat` CPU jiffy parsing + percent-busy from two samples.
//!
//! Percentages need a delta between two readings (jiffy counters are
//! monotonic since boot), so parsing alone never yields a percent — see
//! [`cpu_percent`] and the sampler that keeps the previous reading.

/// One CPU line's jiffy counters (overall `cpu` or a single `cpuN`).
/// Fields match `/proc/stat` column order; only the ones needed for busy% are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    fn parse_fields(rest: &str) -> Self {
        let mut f = rest
            .split_whitespace()
            .map(|v| v.parse::<u64>().unwrap_or(0));
        CpuTimes {
            user: f.next().unwrap_or(0),
            nice: f.next().unwrap_or(0),
            system: f.next().unwrap_or(0),
            idle: f.next().unwrap_or(0),
            iowait: f.next().unwrap_or(0),
            irq: f.next().unwrap_or(0),
            softirq: f.next().unwrap_or(0),
            steal: f.next().unwrap_or(0),
        }
    }

    /// Sum of every counted bucket (the jiffy total for this line).
    pub fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
    }

    /// Idle time including iowait (both count as "not busy").
    pub fn idle_all(&self) -> u64 {
        self.idle + self.iowait
    }
}

/// Parsed `/proc/stat`: the aggregate `cpu` line plus one `cpuN` line per core,
/// in core-index order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProcStat {
    pub overall: CpuTimes,
    pub per_core: Vec<CpuTimes>,
}

/// Parse the `cpu` / `cpuN` lines from `/proc/stat` contents. Unknown or
/// malformed lines are ignored; missing counters default to 0 rather than
/// failing the whole parse (matches the "never fails the page" style used by
/// the rest of the status code — a stat-less sample just reads as 0% busy).
pub fn parse_proc_stat(input: &str) -> ProcStat {
    let mut overall = CpuTimes::default();
    let mut per_core: Vec<(usize, CpuTimes)> = Vec::new();
    for line in input.lines() {
        if !line.starts_with("cpu") {
            continue;
        }
        let Some(first_ws) = line.find(char::is_whitespace) else {
            continue;
        };
        // Data starts after the whole label ("cpu" or "cpuN"), not merely
        // after the "cpu" substring — cpuN's digits sit before the first
        // whitespace and must not be parsed as a data field.
        let label = &line[..first_ws];
        let rest = line[first_ws..].trim_start();
        if label == "cpu" {
            overall = CpuTimes::parse_fields(rest);
        } else if let Some(idx_str) = label.strip_prefix("cpu") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                per_core.push((idx, CpuTimes::parse_fields(rest)));
            }
        }
    }
    per_core.sort_by_key(|(idx, _)| *idx);
    ProcStat {
        overall,
        per_core: per_core.into_iter().map(|(_, t)| t).collect(),
    }
}

/// Busy percent between two samples of the same CPU line (0.0 on first sample
/// or if the counters didn't move, e.g. a core hot-added between reads).
pub fn cpu_percent(prev: &CpuTimes, cur: &CpuTimes) -> f32 {
    let total_delta = cur.total().saturating_sub(prev.total());
    if total_delta == 0 {
        return 0.0;
    }
    let idle_delta = cur.idle_all().saturating_sub(prev.idle_all());
    let busy_delta = total_delta.saturating_sub(idle_delta);
    ((busy_delta as f64 / total_delta as f64) * 100.0).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_1: &str = "\
cpu  100 10 50 800 20 0 5 0 0 0
cpu0 50 5 25 400 10 0 2 0 0 0
cpu1 50 5 25 400 10 0 3 0 0 0
intr 12345
ctxt 6789
btime 1700000000
";
    const SAMPLE_2: &str = "\
cpu  200 10 100 900 30 0 10 0 0 0
cpu0 100 5 50 450 15 0 5 0 0 0
cpu1 100 5 50 450 15 0 5 0 0 0
intr 22345
";

    #[test]
    fn parses_overall_and_per_core_in_order() {
        let stat = parse_proc_stat(SAMPLE_1);
        assert_eq!(stat.overall.user, 100);
        assert_eq!(stat.overall.idle, 800);
        assert_eq!(stat.per_core.len(), 2);
        assert_eq!(stat.per_core[0].user, 50);
        assert_eq!(stat.per_core[1].user, 50);
    }

    #[test]
    fn ignores_non_cpu_lines() {
        let stat = parse_proc_stat(SAMPLE_1);
        // intr/ctxt/btime must not be mistaken for cpuN lines.
        assert_eq!(stat.per_core.len(), 2);
    }

    #[test]
    fn computes_busy_percent_from_two_samples() {
        let a = parse_proc_stat(SAMPLE_1);
        let b = parse_proc_stat(SAMPLE_2);
        let pct = cpu_percent(&a.overall, &b.overall);
        // total delta = (200+10+100+900+30+0+10) - (100+10+50+800+20+0+5) = 1250-985=265
        // idle delta = (900+30)-(800+20) = 110; busy = 265-110=155; pct=155/265*100≈58.49
        assert!((pct - 58.49).abs() < 0.1, "got {pct}");
    }

    #[test]
    fn zero_delta_is_zero_percent_not_a_panic() {
        let a = parse_proc_stat(SAMPLE_1);
        let pct = cpu_percent(&a.overall, &a.overall);
        assert_eq!(pct, 0.0);
    }

    #[test]
    fn empty_input_yields_defaults() {
        let stat = parse_proc_stat("");
        assert_eq!(stat.overall, CpuTimes::default());
        assert!(stat.per_core.is_empty());
    }
}
