//! `/proc/meminfo` + ZFS ARC (`/proc/spl/kstat/zfs/arcstats`) parsing.
//!
//! ARC is parsed separately from meminfo because it is the whole reason this
//! panel breaks memory into segments: ZFS happily grows the ARC to use most
//! of "free" RAM, so a naive `total - free` reads as almost-full even when
//! the ARC would shrink instantly under pressure. Showing it as its own
//! segment (not lumped into "used") is the point.

/// Kilobyte fields from `/proc/meminfo` needed for a used/cache/free breakdown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemInfo {
    pub total_kb: u64,
    pub free_kb: u64,
    pub available_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
    pub sreclaimable_kb: u64,
    pub swap_total_kb: u64,
    pub swap_free_kb: u64,
}

fn kb_value(line: &str) -> Option<u64> {
    // Lines look like "MemTotal:       16374912 kB"
    let rest = line.split(':').nth(1)?;
    rest.split_whitespace().next()?.parse::<u64>().ok()
}

/// Parse `/proc/meminfo` contents. Unknown/missing fields default to 0 so a
/// kernel without some optional counter still renders (just with that bucket
/// at zero) rather than failing the panel.
pub fn parse_meminfo(input: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in input.lines() {
        if let Some(v) = line.strip_prefix("MemTotal:").and_then(|_| kb_value(line)) {
            m.total_kb = v;
        } else if let Some(v) = line.strip_prefix("MemFree:").and_then(|_| kb_value(line)) {
            m.free_kb = v;
        } else if let Some(v) = line
            .strip_prefix("MemAvailable:")
            .and_then(|_| kb_value(line))
        {
            m.available_kb = v;
        } else if let Some(v) = line.strip_prefix("Buffers:").and_then(|_| kb_value(line)) {
            m.buffers_kb = v;
        } else if line.starts_with("Cached:") {
            // Exact "Cached:" prefix (colon included) so "SwapCached:" doesn't match.
            if let Some(v) = kb_value(line) {
                m.cached_kb = v;
            }
        } else if let Some(v) = line
            .strip_prefix("SReclaimable:")
            .and_then(|_| kb_value(line))
        {
            m.sreclaimable_kb = v;
        } else if let Some(v) = line.strip_prefix("SwapTotal:").and_then(|_| kb_value(line)) {
            m.swap_total_kb = v;
        } else if let Some(v) = line.strip_prefix("SwapFree:").and_then(|_| kb_value(line)) {
            m.swap_free_kb = v;
        }
    }
    m
}

/// Parse the ARC "current size" (bytes) out of `arcstats`. Format is
/// `<name> <type> <value>` per line, e.g. `size 4 17179869184`. Returns
/// `None` when the file is absent/unreadable/unparsable (no ZFS, or ARC
/// stats not exposed) — callers treat that as "no ARC to show", not an error.
pub fn parse_arcstats(input: &str) -> Option<u64> {
    for line in input.lines() {
        let mut it = line.split_whitespace();
        let name = it.next()?;
        if name != "size" {
            continue;
        }
        let _kind = it.next();
        return it.next()?.parse::<u64>().ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "\
MemTotal:       16374912 kB
MemFree:         1000000 kB
MemAvailable:    9000000 kB
Buffers:          200000 kB
Cached:          3000000 kB
SwapCached:            0 kB
Active:          5000000 kB
SReclaimable:     500000 kB
SwapTotal:       2097148 kB
SwapFree:        2097148 kB
";

    #[test]
    fn parses_expected_fields() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.total_kb, 16374912);
        assert_eq!(m.free_kb, 1000000);
        assert_eq!(m.available_kb, 9000000);
        assert_eq!(m.buffers_kb, 200000);
        assert_eq!(m.cached_kb, 3000000);
        assert_eq!(m.sreclaimable_kb, 500000);
        assert_eq!(m.swap_total_kb, 2097148);
        assert_eq!(m.swap_free_kb, 2097148);
    }

    #[test]
    fn cached_does_not_swallow_swap_cached() {
        let m = parse_meminfo("Cached:  42 kB\nSwapCached:  7 kB\n");
        assert_eq!(m.cached_kb, 42);
    }

    #[test]
    fn missing_fields_default_to_zero() {
        let m = parse_meminfo("MemTotal: 1000 kB\n");
        assert_eq!(m.total_kb, 1000);
        assert_eq!(m.swap_total_kb, 0);
    }

    #[test]
    fn parses_arc_size_from_kstat_format() {
        let input = "name type data\nhits 4 123\nsize 4 17179869184\nc_max 4 999\n";
        assert_eq!(parse_arcstats(input), Some(17179869184));
    }

    #[test]
    fn missing_arc_file_or_no_size_line_is_none() {
        assert_eq!(parse_arcstats(""), None);
        assert_eq!(parse_arcstats("hits 4 123\n"), None);
    }
}
