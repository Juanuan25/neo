//! Nix repl process helpers: extract files, config fingerprint, stderr error detection.
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Result;

use super::super::registry::{EXTRACT_LIB, NIX_EXTRACTORS};

/// True when `text` looks like a finished Nix evaluation error (not a progress line).
pub(super) fn looks_like_terminal_nix_error(text: &str) -> bool {
    for line in text.lines() {
        let t = line.trim_start();
        // `error:` starts evaluation failures; `error: path …` also appears nested in traces.
        if t.starts_with("error:") || t.contains("error: path ") {
            return true;
        }
    }
    false
}

pub(super) fn trim_for_error(s: &str) -> String {
    let t = s.trim();
    if t.len() <= 4000 {
        t.to_string()
    } else {
        // Prefer the end of the trace (leaf error).
        tail_chars(t, 4000)
    }
}

pub(super) fn tail_chars(s: &str, n: usize) -> String {
    s.chars()
        .rev()
        .take(n)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub(super) fn write_extract_files(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    for e in NIX_EXTRACTORS {
        fs::write(dir.join(e.file_name), e.content)?;
    }
    fs::write(dir.join(EXTRACT_LIB.0), EXTRACT_LIB.1)?;
    Ok(())
}

/// What the evaluated flake sees of the config dir: its `.nix` / `.toml` /
/// `.lock` files (skipping dot entries, `result` and `target`).
///
/// `stat` (path, size, mtime) is cheap and checked on every query; file
/// contents are hashed only when it moves. A git checkout or a touched file
/// with unchanged contents therefore keeps the warm repl, and directory
/// mtimes (any file created anywhere in the tree) no longer count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ConfigFingerprint {
    stat: u64,
    content: u64,
}

impl ConfigFingerprint {
    pub(super) fn of(config_dir: &str) -> Self {
        let files = config_files(Path::new(config_dir));
        Self {
            stat: stat_hash(&files),
            content: content_hash(&files),
        }
    }

    /// Fingerprint now, reusing `self.content` when no file stat changed.
    pub(super) fn current(&self, config_dir: &str) -> Self {
        let files = config_files(Path::new(config_dir));
        let stat = stat_hash(&files);
        if stat == self.stat {
            return *self;
        }
        Self {
            stat,
            content: content_hash(&files),
        }
    }

    /// True when the evaluated inputs differ (stat-only changes do not count).
    pub(super) fn content_differs(&self, other: &Self) -> bool {
        self.content != other.content
    }
}

fn config_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk_config_files(dir, &mut out);
    out.sort();
    out
}

fn walk_config_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "result" || name == "target" {
            continue;
        }
        let path = entry.path();
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_config_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "nix" || e == "toml" || e == "lock")
        {
            out.push(path);
        }
    }
}

fn stat_hash(files: &[PathBuf]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in files {
        p.hash(&mut h);
        if let Ok(meta) = fs::metadata(p) {
            meta.len().hash(&mut h);
            meta.modified().ok().hash(&mut h);
        }
    }
    h.finish()
}

fn content_hash(files: &[PathBuf]) -> u64 {
    let mut h = DefaultHasher::new();
    for p in files {
        p.hash(&mut h);
        // Unreadable hashes as empty; a later readable state then differs.
        fs::read(p).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("neo-fp-{}-{n}", std::process::id()));
        fs::create_dir_all(dir.join("build")).unwrap();
        fs::write(dir.join("settings.toml"), "a = 1\n").unwrap();
        fs::write(dir.join("build/flake.nix"), "{}\n").unwrap();
        dir
    }

    fn bump_mtime(p: &Path) {
        let f = fs::File::options().write(true).open(p).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        f.set_modified(later).unwrap();
    }

    #[test]
    fn touch_and_unrelated_files_keep_fingerprint() {
        let dir = temp_config();
        let d = dir.to_str().unwrap();
        let before = ConfigFingerprint::of(d);

        bump_mtime(&dir.join("settings.toml"));
        fs::write(dir.join("notes.txt"), "x").unwrap();
        fs::create_dir_all(dir.join("result")).unwrap();
        fs::write(dir.join("result/x.nix"), "x").unwrap();
        let after = before.current(d);

        let _ = fs::remove_dir_all(&dir);
        assert!(!before.content_differs(&after));
    }

    #[test]
    fn content_change_new_and_removed_files_differ() {
        let dir = temp_config();
        let d = dir.to_str().unwrap();
        let base = ConfigFingerprint::of(d);

        fs::write(dir.join("settings.toml"), "a = 2\n").unwrap();
        let edited = base.current(d);
        fs::write(dir.join("build/extra.nix"), "{}").unwrap();
        let added = edited.current(d);
        fs::remove_file(dir.join("build/extra.nix")).unwrap();
        let removed = added.current(d);

        let _ = fs::remove_dir_all(&dir);
        assert!(base.content_differs(&edited));
        assert!(edited.content_differs(&added));
        assert!(added.content_differs(&removed));
        assert!(!edited.content_differs(&removed));
    }
}
