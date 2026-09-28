//! Data model for the pending-changes / version-compare diff preview.

/// Per-file change status (from `--name-status` / porcelain status).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Unmerged,
}

impl FileStatus {
    /// Parse the leading status letter of a `--name-status` record (`R100` → Renamed).
    pub fn from_name_status(code: &str) -> Option<Self> {
        match code.chars().next()? {
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'T' => Some(Self::TypeChanged),
            'U' => Some(Self::Unmerged),
            _ => None,
        }
    }

    /// Short status letter shown next to the color (never color-only).
    pub fn letter(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChanged => "T",
            Self::Untracked => "U",
            Self::Unmerged => "!",
        }
    }

    /// Human label (tooltips / screen readers).
    pub fn label(self) -> &'static str {
        match self {
            Self::Added => "Added",
            Self::Modified => "Modified",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Copied => "Copied",
            Self::TypeChanged => "Type changed",
            Self::Untracked => "Untracked (new)",
            Self::Unmerged => "Conflict",
        }
    }

    /// Stable key for CSS (`data-status`).
    pub fn css_key(self) -> &'static str {
        match self {
            Self::Added | Self::Untracked => "added",
            Self::Modified | Self::TypeChanged => "modified",
            Self::Deleted | Self::Unmerged => "deleted",
            Self::Renamed | Self::Copied => "renamed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Del,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
    /// Followed by `\ No newline at end of file`.
    pub no_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// Text after the closing `@@` (function / section context).
    pub section: String,
    pub lines: Vec<DiffLine>,
}

/// One file section of a unified `git diff` patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilePatch {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: Option<FileStatus>,
    pub binary: bool,
    pub similarity: Option<u8>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub hunks: Vec<Hunk>,
}

impl FilePatch {
    /// Path used to match against `--name-status` / `--numstat` records.
    pub fn key_path(&self) -> Option<&str> {
        self.new_path.as_deref().or(self.old_path.as_deref())
    }
}

/// A changed file with everything the preview renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    /// Current path (old path for deletions).
    pub path: String,
    /// Previous path for renames / copies.
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    pub similarity: Option<u8>,
    /// Mode change (`100644 → 100755`) if any.
    pub mode_change: Option<(String, String)>,
    pub hunks: Vec<Hunk>,
    /// Content was not loaded (too large / unreadable); reason for the placeholder.
    pub omitted: Option<String>,
}

/// All changes of one preview (pending worktree or rev → rev).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub files: Vec<FileChange>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn additions(&self) -> u32 {
        self.files.iter().map(|f| f.additions).sum()
    }

    pub fn deletions(&self) -> u32 {
        self.files.iter().map(|f| f.deletions).sum()
    }
}
