//! Parsers for `git status --porcelain=v1 -z`, `git diff --numstat -z`,
//! `git diff --name-status -z` and unified `git diff` patches.
//!
//! All parsers are total: malformed records are skipped instead of failing, so a
//! surprising git output degrades the preview rather than breaking it.
use super::model::{DiffLine, FilePatch, FileStatus, Hunk, LineKind};

/// One record of `git status --porcelain=v1 -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Index (staged) status column.
    pub x: char,
    /// Worktree status column.
    pub y: char,
    pub path: String,
    /// Source path for renames / copies.
    pub orig_path: Option<String>,
}

impl StatusEntry {
    pub fn is_untracked(&self) -> bool {
        self.x == '?' && self.y == '?'
    }
}

/// One record of `git diff --numstat -z` (`None` counts = binary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumstatEntry {
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub path: String,
    pub old_path: Option<String>,
}

/// One record of `git diff --name-status -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameStatusEntry {
    pub status: FileStatus,
    pub score: Option<u8>,
    pub path: String,
    pub old_path: Option<String>,
}

/// Parse `git status --porcelain=v1 -z` (`XY PATH\0[ORIG\0]`).
pub fn parse_status_z(raw: &str) -> Vec<StatusEntry> {
    let mut out = Vec::new();
    let mut it = raw.split('\0');
    while let Some(rec) = it.next() {
        if rec.len() < 4 || !rec.is_char_boundary(3) {
            continue;
        }
        let mut chars = rec.chars();
        let (Some(x), Some(y)) = (chars.next(), chars.next()) else {
            continue;
        };
        let path = rec[3..].to_string();
        let orig_path = if matches!(x, 'R' | 'C') || matches!(y, 'R' | 'C') {
            it.next().filter(|s| !s.is_empty()).map(str::to_string)
        } else {
            None
        };
        out.push(StatusEntry {
            x,
            y,
            path,
            orig_path,
        });
    }
    out
}

/// Parse `git diff --numstat -z`.
///
/// Normal records are `ADD\tDEL\tPATH\0`; renames are `ADD\tDEL\t\0OLD\0NEW\0`.
pub fn parse_numstat_z(raw: &str) -> Vec<NumstatEntry> {
    let mut out = Vec::new();
    let mut it = raw.split('\0');
    while let Some(rec) = it.next() {
        let rec = rec.trim_start_matches('\n');
        if rec.is_empty() {
            continue;
        }
        let mut parts = rec.splitn(3, '\t');
        let (Some(a), Some(d), Some(p)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let (path, old_path) = if p.is_empty() {
            let (Some(old), Some(new)) = (it.next(), it.next()) else {
                break;
            };
            (new.to_string(), Some(old.to_string()))
        } else {
            (p.to_string(), None)
        };
        out.push(NumstatEntry {
            additions: a.parse().ok(),
            deletions: d.parse().ok(),
            path,
            old_path,
        });
    }
    out
}

/// Parse `git diff --name-status -z` (`M\0PATH\0`, `R087\0OLD\0NEW\0`).
pub fn parse_name_status_z(raw: &str) -> Vec<NameStatusEntry> {
    let mut out = Vec::new();
    let mut it = raw.split('\0');
    while let Some(code) = it.next() {
        let code = code.trim_start_matches('\n');
        if code.is_empty() {
            continue;
        }
        let Some(status) = FileStatus::from_name_status(code) else {
            // Unknown code: consume its path so we stay aligned.
            let _ = it.next();
            continue;
        };
        let score = code.get(1..).and_then(|s| s.parse::<u8>().ok());
        let (path, old_path) = if matches!(status, FileStatus::Renamed | FileStatus::Copied) {
            let (Some(old), Some(new)) = (it.next(), it.next()) else {
                break;
            };
            (new.to_string(), Some(old.to_string()))
        } else {
            let Some(p) = it.next() else {
                break;
            };
            (p.to_string(), None)
        };
        out.push(NameStatusEntry {
            status,
            score,
            path,
            old_path,
        });
    }
    out
}

/// Undo git's C-style path quoting (`"a\tb\303\244"`). Unquoted input is returned as-is.
pub fn unquote_path(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|r| r.strip_suffix('"')) else {
        return s.to_string();
    };
    let bytes = inner.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'\\' || i + 1 >= bytes.len() {
            out.push(b);
            i += 1;
            continue;
        }
        let n = bytes[i + 1];
        match n {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'0'..=b'7' => {
                let end = (i + 4).min(bytes.len());
                let oct = &inner[i + 1..end];
                if oct.len() == 3 && oct.bytes().all(|c| (b'0'..=b'7').contains(&c)) {
                    if let Ok(v) = u8::from_str_radix(oct, 8) {
                        out.push(v);
                        i += 4;
                        continue;
                    }
                }
                out.push(n);
            }
            other => out.push(other),
        }
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Path from a `--- a/x` / `+++ b/x` line (`/dev/null` → `None`).
fn diff_side_path(raw: &str) -> Option<String> {
    let raw = raw.strip_suffix('\t').unwrap_or(raw);
    if raw == "/dev/null" {
        return None;
    }
    let p = unquote_path(raw);
    let stripped = p
        .strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .map(str::to_string);
    Some(stripped.unwrap_or(p))
}

/// Best-effort `(old, new)` from `diff --git a/X b/Y` (ambiguous with spaces; used as fallback).
fn header_paths(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"') {
        // Quoted first path: find its closing quote (skipping escapes).
        let bytes = rest.as_bytes();
        let mut i = 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => break,
                _ => i += 1,
            }
        }
        let end = (i + 1).min(rest.len());
        let a = diff_side_path(&rest[..end]);
        let b = diff_side_path(rest[end..].trim_start());
        return (a, b);
    }
    if let Some(body) = rest.strip_prefix("a/") {
        // Same path on both sides: "a/P b/P" → the halves are equal.
        let total = body.len();
        if total >= 3 {
            let half = (total - 3) / 2;
            if body.is_char_boundary(half) && body.get(half..half + 3) == Some(" b/") {
                let (l, r) = (&body[..half], &body[half + 3..]);
                if l == r {
                    return (Some(l.to_string()), Some(r.to_string()));
                }
            }
        }
        if let Some(idx) = body.find(" b/") {
            return (
                Some(body[..idx].to_string()),
                Some(body[idx + 3..].to_string()),
            );
        }
        if let Some(idx) = body.find(" \"b/") {
            return (
                Some(body[..idx].to_string()),
                diff_side_path(&body[idx + 1..]),
            );
        }
    }
    (None, None)
}

/// Parse `@@ -a[,b] +c[,d] @@ section`.
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ ")?;
    let close = rest.find(" @@")?;
    let ranges = &rest[..close];
    let section = rest[close + 3..].trim_start().to_string();
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((s, l)) => Some((s.parse().ok()?, l.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let (old_start, old_len) = range(old)?;
    let (new_start, new_len) = range(new)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        section,
        lines: Vec::new(),
    })
}

struct PatchBuilder {
    patch: FilePatch,
    header_old: Option<String>,
    header_new: Option<String>,
    seen_minus: bool,
    seen_plus: bool,
    old_left: u32,
    new_left: u32,
    old_no: u32,
    new_no: u32,
}

impl PatchBuilder {
    fn new(header_rest: &str) -> Self {
        let (header_old, header_new) = header_paths(header_rest);
        Self {
            patch: FilePatch::default(),
            header_old,
            header_new,
            seen_minus: false,
            seen_plus: false,
            old_left: 0,
            new_left: 0,
            old_no: 0,
            new_no: 0,
        }
    }

    fn in_hunk(&self) -> bool {
        (self.old_left > 0 || self.new_left > 0) && !self.patch.hunks.is_empty()
    }

    fn push_line(&mut self, kind: LineKind, text: &str) {
        let (old_no, new_no) = match kind {
            LineKind::Context => {
                self.old_left = self.old_left.saturating_sub(1);
                self.new_left = self.new_left.saturating_sub(1);
                (Some(self.old_no), Some(self.new_no))
            }
            LineKind::Del => {
                self.old_left = self.old_left.saturating_sub(1);
                (Some(self.old_no), None)
            }
            LineKind::Add => {
                self.new_left = self.new_left.saturating_sub(1);
                (None, Some(self.new_no))
            }
        };
        if old_no.is_some() {
            self.old_no += 1;
        }
        if new_no.is_some() {
            self.new_no += 1;
        }
        if let Some(h) = self.patch.hunks.last_mut() {
            h.lines.push(DiffLine {
                kind,
                old_no,
                new_no,
                text: text.to_string(),
                no_newline: false,
            });
        }
    }

    fn mark_no_newline(&mut self) {
        if let Some(l) = self.patch.hunks.last_mut().and_then(|h| h.lines.last_mut()) {
            l.no_newline = true;
        }
    }

    fn finish(mut self) -> FilePatch {
        if !self.seen_minus && self.patch.old_path.is_none() {
            self.patch.old_path = self.header_old.take();
        }
        if !self.seen_plus && self.patch.new_path.is_none() {
            self.patch.new_path = self.header_new.take();
        }
        match self.patch.status {
            Some(FileStatus::Added) => self.patch.old_path = None,
            Some(FileStatus::Deleted) => self.patch.new_path = None,
            _ => {}
        }
        self.patch
    }
}

/// Parse a unified `git diff` patch (one [`FilePatch`] per `diff --git` section).
pub fn parse_patch(text: &str) -> Vec<FilePatch> {
    let mut out = Vec::new();
    let mut cur: Option<PatchBuilder> = None;
    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(b) = cur.take() {
                out.push(b.finish());
            }
            cur = Some(PatchBuilder::new(rest));
            continue;
        }
        let Some(b) = cur.as_mut() else {
            continue;
        };
        if b.in_hunk() {
            match line.as_bytes().first() {
                Some(b' ') => b.push_line(LineKind::Context, &line[1..]),
                Some(b'+') => b.push_line(LineKind::Add, &line[1..]),
                Some(b'-') => b.push_line(LineKind::Del, &line[1..]),
                Some(b'\\') => b.mark_no_newline(),
                // Some tools strip the lone space of empty context lines.
                None => b.push_line(LineKind::Context, ""),
                Some(_) => {
                    b.old_left = 0;
                    b.new_left = 0;
                }
            }
            continue;
        }
        if line.starts_with('\\') {
            b.mark_no_newline();
        } else if line.starts_with("@@ ") {
            if let Some(h) = parse_hunk_header(line) {
                b.old_left = h.old_len;
                b.new_left = h.new_len;
                b.old_no = h.old_start;
                b.new_no = h.new_start;
                b.patch.hunks.push(h);
            }
        } else if let Some(m) = line.strip_prefix("new file mode ") {
            b.patch.status = Some(FileStatus::Added);
            b.patch.new_mode = Some(m.trim().to_string());
        } else if let Some(m) = line.strip_prefix("deleted file mode ") {
            b.patch.status = Some(FileStatus::Deleted);
            b.patch.old_mode = Some(m.trim().to_string());
        } else if let Some(m) = line.strip_prefix("old mode ") {
            b.patch.old_mode = Some(m.trim().to_string());
        } else if let Some(m) = line.strip_prefix("new mode ") {
            b.patch.new_mode = Some(m.trim().to_string());
        } else if let Some(s) = line.strip_prefix("similarity index ") {
            b.patch.similarity = s.trim().trim_end_matches('%').parse().ok();
        } else if let Some(p) = line.strip_prefix("rename from ") {
            b.patch.status = Some(FileStatus::Renamed);
            b.patch.old_path = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            b.patch.status = Some(FileStatus::Renamed);
            b.patch.new_path = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("copy from ") {
            b.patch.status = Some(FileStatus::Copied);
            b.patch.old_path = Some(unquote_path(p));
        } else if let Some(p) = line.strip_prefix("copy to ") {
            b.patch.status = Some(FileStatus::Copied);
            b.patch.new_path = Some(unquote_path(p));
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            b.patch.binary = true;
        } else if let Some(p) = line.strip_prefix("--- ") {
            b.seen_minus = true;
            b.patch.old_path = diff_side_path(p);
        } else if let Some(p) = line.strip_prefix("+++ ") {
            b.seen_plus = true;
            b.patch.new_path = diff_side_path(p);
        }
    }
    if let Some(b) = cur.take() {
        out.push(b.finish());
    }
    for p in &mut out {
        if p.status.is_none() {
            p.status = Some(match (&p.old_path, &p.new_path) {
                (None, Some(_)) => FileStatus::Added,
                (Some(_), None) => FileStatus::Deleted,
                _ => FileStatus::Modified,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_z_handles_renames_and_untracked() {
        let raw = " M settings.toml\0R  new name.nix\0old name.nix\0?? plugins/a b.txt\0";
        let e = parse_status_z(raw);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].x, ' ');
        assert_eq!(e[0].y, 'M');
        assert_eq!(e[0].path, "settings.toml");
        assert_eq!(e[1].path, "new name.nix");
        assert_eq!(e[1].orig_path.as_deref(), Some("old name.nix"));
        assert!(e[2].is_untracked());
        assert_eq!(e[2].path, "plugins/a b.txt");
    }

    #[test]
    fn numstat_z_binary_and_rename() {
        let raw = "3\t1\tsettings.toml\0-\t-\timg.png\0\
                   0\t0\t\0old.nix\0new.nix\0";
        let e = parse_numstat_z(raw);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0].additions, Some(3));
        assert_eq!(e[0].deletions, Some(1));
        assert_eq!(e[1].additions, None);
        assert_eq!(e[1].path, "img.png");
        assert_eq!(e[2].path, "new.nix");
        assert_eq!(e[2].old_path.as_deref(), Some("old.nix"));
    }

    #[test]
    fn name_status_z_all_kinds() {
        let raw = "M\0settings.toml\0A\0flake.lock\0D\0gone.txt\0R087\0a.nix\0b.nix\0";
        let e = parse_name_status_z(raw);
        assert_eq!(e.len(), 4);
        assert_eq!(e[0].status, FileStatus::Modified);
        assert_eq!(e[1].status, FileStatus::Added);
        assert_eq!(e[2].status, FileStatus::Deleted);
        assert_eq!(e[3].status, FileStatus::Renamed);
        assert_eq!(e[3].score, Some(87));
        assert_eq!(e[3].old_path.as_deref(), Some("a.nix"));
        assert_eq!(e[3].path, "b.nix");
    }

    #[test]
    fn unquote_c_style_paths() {
        assert_eq!(unquote_path("plain.txt"), "plain.txt");
        assert_eq!(unquote_path(r#""a\tb""#), "a\tb");
        assert_eq!(unquote_path(r#""\303\244.txt""#), "ä.txt");
        assert_eq!(unquote_path(r#""q\"x""#), "q\"x");
    }

    const PATCH: &str = "diff --git a/settings.toml b/settings.toml
index 1111111..2222222 100644
--- a/settings.toml
+++ b/settings.toml
@@ -1,4 +1,5 @@ [core]
 [core]
-hostname = \"old\"
+hostname = \"new\"
+timeZone = \"UTC\"

 [services.foo]
@@ -10,2 +11,2 @@
 a = 1
-b = 2
\\ No newline at end of file
+b = 3
\\ No newline at end of file
diff --git a/img.png b/img.png
new file mode 100644
index 0000000..3333333
Binary files /dev/null and b/img.png differ
diff --git a/old.nix b/new.nix
similarity index 100%
rename from old.nix
rename to new.nix
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 4444444..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
";

    #[test]
    fn patch_parses_hunks_line_numbers_and_markers() {
        let p = parse_patch(PATCH);
        assert_eq!(p.len(), 5);

        let s = &p[0];
        assert_eq!(s.status, Some(FileStatus::Modified));
        assert_eq!(s.new_path.as_deref(), Some("settings.toml"));
        assert_eq!(s.hunks.len(), 2);
        let h = &s.hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (1, 4, 1, 5)
        );
        assert_eq!(h.section, "[core]");
        assert_eq!(h.lines.len(), 6);
        assert_eq!(h.lines[1].kind, LineKind::Del);
        assert_eq!(h.lines[1].old_no, Some(2));
        assert_eq!(h.lines[1].new_no, None);
        assert_eq!(h.lines[3].kind, LineKind::Add);
        assert_eq!(h.lines[3].new_no, Some(3));
        // Empty context line (" ") keeps numbering.
        assert_eq!(h.lines[4].text, "");
        assert_eq!(h.lines[4].old_no, Some(3));
        assert_eq!(h.lines[5].new_no, Some(5));
        let h2 = &s.hunks[1];
        assert!(h2.lines[1].no_newline);
        assert!(h2.lines[2].no_newline);
        assert_eq!(h2.lines[2].new_no, Some(12));

        let bin = &p[1];
        assert!(bin.binary);
        assert_eq!(bin.status, Some(FileStatus::Added));
        assert_eq!(bin.old_path, None);
        assert_eq!(bin.new_path.as_deref(), Some("img.png"));

        let ren = &p[2];
        assert_eq!(ren.status, Some(FileStatus::Renamed));
        assert_eq!(ren.similarity, Some(100));
        assert_eq!(ren.old_path.as_deref(), Some("old.nix"));
        assert_eq!(ren.new_path.as_deref(), Some("new.nix"));
        assert!(ren.hunks.is_empty());

        let del = &p[3];
        assert_eq!(del.status, Some(FileStatus::Deleted));
        assert_eq!(del.key_path(), Some("gone.txt"));
        assert_eq!(del.hunks[0].lines[0].kind, LineKind::Del);

        let mode = &p[4];
        assert_eq!(mode.status, Some(FileStatus::Modified));
        assert_eq!(mode.old_mode.as_deref(), Some("100644"));
        assert_eq!(mode.new_mode.as_deref(), Some("100755"));
        assert_eq!(mode.new_path.as_deref(), Some("run.sh"));
    }

    #[test]
    fn patch_line_starting_with_diff_markers_inside_hunk() {
        // A removed line "-- x" and an added "++ y" must stay hunk lines, not headers.
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n--- x\n+++ y\n";
        let p = parse_patch(patch);
        assert_eq!(p.len(), 1);
        let l = &p[0].hunks[0].lines;
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].text, "-- x");
        assert_eq!(l[1].text, "++ y");
    }

    #[test]
    fn header_paths_with_spaces() {
        assert_eq!(
            header_paths("a/my file b/my file"),
            (Some("my file".into()), Some("my file".into()))
        );
        assert_eq!(
            header_paths("\"a/x\\ty\" \"b/x\\ty\""),
            (Some("x\ty".into()), Some("x\ty".into()))
        );
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse_patch("warning: something\nnot a diff\n").is_empty());
        assert!(parse_status_z("x\0").is_empty());
        assert!(parse_numstat_z("garbage").is_empty());
    }
}
