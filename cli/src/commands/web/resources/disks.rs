//! `/proc/mounts` filtering + `statvfs(2)` usage for real filesystems (ZFS
//! pools/datasets included). No shelling out to `df`/`zfs`: `statvfs` is a
//! plain libc call on a path we already trust (parsed straight from the
//! kernel's mount table), so it works under the same restricted PATH/sandbox
//! the rest of neo-web runs under.

use std::ffi::CString;
use std::mem::MaybeUninit;

/// One real (non-virtual) mount from `/proc/mounts`.
#[derive(Clone, Debug, PartialEq)]
pub struct MountInfo {
    pub device: String,
    pub mount_point: String,
    pub fs_type: String,
}

/// Filesystem types worth showing usage for. Deliberately excludes
/// pseudo/virtual filesystems (proc, sysfs, tmpfs, cgroup, overlay, ...) that
/// either aren't real storage or would otherwise flood the list with every
/// docker overlay2 layer mount.
const REAL_FS_TYPES: &[&str] = &[
    "zfs", "ext2", "ext3", "ext4", "xfs", "btrfs", "vfat", "exfat", "ntfs", "ntfs3", "f2fs",
    "reiserfs", "jfs", "hfsplus",
];

/// Parse `/proc/mounts` (or `/proc/self/mounts`), keeping only entries whose
/// fs type is in [`REAL_FS_TYPES`]. Octal escapes (`\040` for space, etc.) in
/// paths are left as-is — mount points containing spaces are rare enough on a
/// homeserver root/zfs layout that decoding isn't worth the complexity here.
pub fn parse_proc_mounts(input: &str) -> Vec<MountInfo> {
    let mut out = Vec::new();
    for line in input.lines() {
        let mut f = line.split_whitespace();
        let (Some(device), Some(mount_point), Some(fs_type)) = (f.next(), f.next(), f.next())
        else {
            continue;
        };
        if REAL_FS_TYPES.contains(&fs_type) {
            out.push(MountInfo {
                device: device.to_string(),
                mount_point: mount_point.to_string(),
                fs_type: fs_type.to_string(),
            });
        }
    }
    out
}

/// Total/used bytes for a mounted filesystem.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UsageBytes {
    pub total: u64,
    pub used: u64,
    pub avail: u64,
}

/// `statvfs(2)` on `path`. Returns `None` on any error (path gone, permission
/// denied, non-UTF8 with an embedded NUL) rather than failing the panel.
pub fn statvfs_usage(path: &str) -> Option<UsageBytes> {
    let c_path = CString::new(path).ok()?;
    let mut buf: MaybeUninit<libc::statvfs> = MaybeUninit::uninit();
    // SAFETY: buf is a valid, appropriately-sized out pointer for statvfs;
    // it's only read after the call succeeds.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), buf.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so the kernel filled in `buf`.
    let s = unsafe { buf.assume_init() };
    // libc's statvfs fields are already u64 on Linux (all supported ABIs), so
    // no cast is needed here.
    let frsize = if s.f_frsize > 0 {
        s.f_frsize
    } else {
        s.f_bsize
    };
    let total = frsize.saturating_mul(s.f_blocks);
    let avail = frsize.saturating_mul(s.f_bavail);
    // f_bfree includes root-reserved blocks; use f_bavail for both "avail"
    // and to derive "used" so the numbers match what a non-root user sees.
    let used = total.saturating_sub(avail);
    Some(UsageBytes { total, used, avail })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOUNTS: &str = "\
rpool/root / zfs rw,relatime,xattr,noacl 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
tmpfs /run tmpfs rw,nosuid,nodev,size=1635536k,mode=755 0 0
rpool/nix /nix zfs rw,relatime,xattr,noacl 0 0
/dev/sda1 /boot vfat rw,relatime 0 0
overlay /var/lib/docker/overlay2/abc123/merged overlay rw,relatime 0 0
cgroup2 /sys/fs/cgroup cgroup2 rw,nosuid,nodev,noexec,relatime 0 0
";

    #[test]
    fn keeps_only_real_filesystems() {
        let mounts = parse_proc_mounts(MOUNTS);
        let types: Vec<&str> = mounts.iter().map(|m| m.fs_type.as_str()).collect();
        assert_eq!(types, vec!["zfs", "zfs", "vfat"]);
    }

    #[test]
    fn keeps_device_and_mount_point() {
        let mounts = parse_proc_mounts(MOUNTS);
        assert_eq!(mounts[0].device, "rpool/root");
        assert_eq!(mounts[0].mount_point, "/");
        assert_eq!(mounts[1].mount_point, "/nix");
    }

    #[test]
    fn excludes_overlay_and_pseudo_filesystems() {
        let mounts = parse_proc_mounts(MOUNTS);
        assert!(!mounts.iter().any(|m| m.fs_type == "overlay"));
        assert!(!mounts.iter().any(|m| m.fs_type == "proc"));
        assert!(!mounts.iter().any(|m| m.fs_type == "tmpfs"));
        assert!(!mounts.iter().any(|m| m.fs_type == "cgroup2"));
    }

    #[test]
    fn empty_input_is_empty() {
        assert!(parse_proc_mounts("").is_empty());
    }

    #[test]
    fn statvfs_on_root_succeeds_and_is_self_consistent() {
        let usage = statvfs_usage("/").expect("statvfs(/) should work in test sandboxes");
        assert!(usage.total > 0);
        assert_eq!(usage.used, usage.total - usage.avail);
    }

    #[test]
    fn statvfs_on_missing_path_is_none() {
        assert_eq!(
            statvfs_usage("/definitely/does/not/exist/neo-test-path"),
            None
        );
    }
}
