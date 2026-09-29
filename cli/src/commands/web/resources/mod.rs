//! System resource panel: `/proc`-only sampling (CPU, memory incl. ZFS ARC,
//! disks/pools, network, load, uptime, top processes) for the services
//! overview's resources widget. No `sysinfo`/`systemstat` crate: the fields
//! we need are a handful of `/proc` files plus one libc `statvfs` call for
//! disk usage, all things this module already has to parse itself to get the
//! ZFS ARC breakdown (no crate exposes that), so pulling in a general-purpose
//! system-info crate would add a dependency without removing any of this
//! code — see `mem.rs` for why ARC needs its own parsing regardless.
mod cpu;
mod disks;
mod mem;
mod net;
mod procs;
mod sampler;
mod snapshot;

pub use sampler::ResourceSampler;
pub use snapshot::Snapshot;
