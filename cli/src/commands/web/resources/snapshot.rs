//! JSON shape sent to the browser by `GET /resources/snapshot`. Kept as raw
//! numbers (bytes, percents, ticks) rather than pre-formatted strings —
//! formatting (units, thresholds, colors) is the client's job so it can
//! re-render without a round trip on things like a sort-column change.

use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct CpuSnapshot {
    pub overall_pct: f32,
    pub per_core_pct: Vec<f32>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemSnapshot {
    pub total_bytes: u64,
    /// Everything else: total - free - cache/buffers - arc (floored at 0).
    pub used_bytes: u64,
    /// Reclaimable page cache + buffers (not ARC — ARC is its own segment
    /// below, that's the whole point of splitting this out for ZFS hosts).
    pub cache_bytes: u64,
    /// `None` when `/proc/spl/kstat/zfs/arcstats` isn't present (no ZFS / ARC
    /// stats not exposed) — the client hides the segment entirely rather
    /// than showing a misleading 0.
    pub arc_bytes: Option<u64>,
    pub free_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiskSnapshot {
    pub mount: String,
    pub fs_type: String,
    pub is_zfs: bool,
    pub total_bytes: u64,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct NetSnapshot {
    pub rx_bytes_per_sec: f64,
    pub tx_bytes_per_sec: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProcSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_pct: f32,
    pub mem_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Snapshot {
    pub uptime_secs: u64,
    /// 1/5/15-minute load averages, straight from `/proc/loadavg`.
    pub load: [f32; 3],
    pub cpu: CpuSnapshot,
    pub mem: MemSnapshot,
    pub disks: Vec<DiskSnapshot>,
    pub net: NetSnapshot,
    /// Union of the top-N by CPU and top-N by memory (see the sampler); the
    /// client sorts/truncates this client-side so switching the sort column
    /// doesn't need another request.
    pub procs: Vec<ProcSnapshot>,
}
