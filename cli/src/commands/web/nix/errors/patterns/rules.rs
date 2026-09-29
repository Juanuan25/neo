// Ordered match rules for Nix error classification.
use super::super::NixErrorKind;

pub(super) struct Rule {
    /// Lower number = higher priority (ties: first rule wins).
    pub priority: u8,
    pub kind: NixErrorKind,
    /// Matches when all needles of any one group appear (case-insensitive substrings).
    pub any: &'static [&'static [&'static str]],
    /// Optional extra predicate on the full text.
    pub extra: Option<fn(&str) -> bool>,
    pub summary: Summary,
}

/// One-line summary for a matched rule.
pub(super) enum Summary {
    Text(&'static str),
    /// Built from the full text and the store paths found in it.
    With(fn(&str, &[String]) -> String),
}

pub(super) fn summary_missing_path(_text: &str, paths: &[String]) -> String {
    match paths.first() {
        Some(p) => {
            format!("Store path is missing (often after GC or a lock from another machine): {p}")
        }
        None => "A referenced Nix store path does not exist".to_string(),
    }
}

pub(super) fn summary_hash(_text: &str, paths: &[String]) -> String {
    match paths.first() {
        Some(p) => format!("Fixed-output hash mismatch for {p}"),
        None => "Fixed-output derivation hash mismatch".to_string(),
    }
}

pub(super) fn summary_lock(_text: &str, paths: &[String]) -> String {
    match paths.first() {
        Some(p) => format!(
            "flake.lock appears to reference a vanished path ({p}); update inputs or re-lock"
        ),
        None => "flake.lock references a path that is not available on this machine".to_string(),
    }
}

pub(super) fn summary_unknown(text: &str, _paths: &[String]) -> String {
    // Prefer the last `error:` line as summary.
    let leaf = text
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("error:"))
        .map(|l| l.trim())
        .unwrap_or_else(|| {
            let t = text.trim();
            if t.len() > 200 {
                // first 200 chars
                let end = t.char_indices().nth(200).map(|(i, _)| i).unwrap_or(t.len());
                &t[..end]
            } else {
                t
            }
        });
    if leaf.is_empty() {
        "Unknown Nix evaluation error".to_string()
    } else {
        leaf.to_string()
    }
}

fn in_store(t: &str) -> bool {
    t.contains("/nix/store/")
}

fn lower_has_any(t: &str, words: &[&str]) -> bool {
    let l = t.to_lowercase();
    words.iter().any(|w| l.contains(w))
}

const NETWORK: Summary = Summary::Text(
    "Failed to download a flake input or fixed-output source (network or remote error)",
);
const SSL: Summary = Summary::Text("TLS/SSL failure while fetching a remote flake input");
const SQLITE: Summary =
    Summary::Text("Nix database is busy or locked (another nix process may be running)");
const LOCK: Summary = Summary::With(summary_lock);

pub(super) const RULES: &[Rule] = &[
    Rule {
        priority: 10,
        kind: NixErrorKind::MissingStorePath,
        any: &[
            &["does not exist"],
            &["no such file or directory"],
            &["is not valid"],
        ],
        extra: Some(in_store),
        summary: Summary::With(summary_missing_path),
    },
    Rule {
        priority: 15,
        kind: NixErrorKind::HashMismatch,
        any: &[&["hash mismatch"], &["narhashmismatch"]],
        extra: None,
        summary: Summary::With(summary_hash),
    },
    Rule {
        priority: 15,
        kind: NixErrorKind::HashMismatch,
        any: &[&["specified:", "got:"]],
        extra: Some(|t| lower_has_any(t, &["hash", "nar"]) || t.contains("sha256")),
        summary: Summary::With(summary_hash),
    },
    Rule {
        priority: 15,
        kind: NixErrorKind::HashMismatch,
        any: &[&["fixed-output derivation produced path"]],
        extra: Some(|t| lower_has_any(t, &["hash"])),
        summary: Summary::With(summary_hash),
    },
    Rule {
        priority: 18,
        kind: NixErrorKind::NetworkFetchFailed,
        any: &[
            &["unable to download"],
            &["http error"],
            &["connection refused"],
            &["could not download"],
            &["failed to fetch"],
            &["network is unreachable"],
            &["temporary failure in name resolution"],
            &["could not resolve host"],
        ],
        extra: None,
        summary: NETWORK,
    },
    Rule {
        priority: 18,
        kind: NixErrorKind::NetworkFetchFailed,
        any: &[&["ssl"]],
        extra: Some(|t| lower_has_any(t, &["certificate", "handshake", "tls"])),
        summary: SSL,
    },
    Rule {
        priority: 18,
        kind: NixErrorKind::NetworkFetchFailed,
        any: &[&["tls"]],
        extra: Some(|t| lower_has_any(t, &["error", "handshake", "certificate"])),
        summary: SSL,
    },
    Rule {
        priority: 22,
        kind: NixErrorKind::PermissionDenied,
        any: &[&["sqlite database is busy"]],
        extra: None,
        summary: SQLITE,
    },
    Rule {
        priority: 22,
        kind: NixErrorKind::PermissionDenied,
        any: &[&["database is locked"]],
        extra: Some(|t| lower_has_any(t, &["sqlite", "nix"])),
        summary: SQLITE,
    },
    Rule {
        priority: 25,
        kind: NixErrorKind::InfiniteRecursion,
        any: &[&["infinite recursion"], &["circular import"]],
        extra: None,
        summary: Summary::Text("Nix hit infinite recursion while evaluating the configuration"),
    },
    Rule {
        priority: 25,
        kind: NixErrorKind::UndefinedVariable,
        any: &[&["undefined variable"]],
        extra: None,
        summary: Summary::Text("Undefined variable in the Nix configuration"),
    },
    Rule {
        priority: 25,
        kind: NixErrorKind::PermissionDenied,
        any: &[&["permission denied"], &["operation not permitted"]],
        extra: None,
        summary: Summary::Text("Permission denied while accessing the Nix store or files"),
    },
    Rule {
        priority: 28,
        kind: NixErrorKind::EvalAssertion,
        any: &[
            &["does not provide attribute"],
            &["flake '", "does not provide"],
        ],
        extra: None,
        summary: Summary::Text(
            "Flake does not provide the expected attribute (check flake outputs / inputs)",
        ),
    },
    Rule {
        priority: 28,
        kind: NixErrorKind::EvalAssertion,
        any: &[&["cannot import"]],
        extra: None,
        summary: Summary::Text(
            "Nix could not import a file or module referenced by the configuration",
        ),
    },
    Rule {
        priority: 28,
        kind: NixErrorKind::EvalAssertion,
        any: &[
            &["cannot coerce"],
            &["value is a function while a set was expected"],
            &["value is a string while a set was expected"],
        ],
        extra: None,
        summary: Summary::Text(
            "Type error during evaluation (cannot coerce or unexpected value type)",
        ),
    },
    Rule {
        priority: 30,
        kind: NixErrorKind::EvalAssertion,
        any: &[&["assertion"], &["error: throw"]],
        extra: None,
        summary: Summary::Text("A Nix assertion or throw failed during evaluation"),
    },
    Rule {
        priority: 30,
        kind: NixErrorKind::Timeout,
        any: &[&["timeout waiting for marker"]],
        extra: None,
        summary: Summary::Text("Nix evaluation timed out waiting for a result marker"),
    },
    Rule {
        priority: 30,
        kind: NixErrorKind::ProcessDied,
        any: &[&["stdout closed"]],
        extra: None,
        summary: Summary::Text("The nix repl process exited unexpectedly"),
    },
    // path:/ git+file: locks from another machine (often without flake.lock in the message).
    Rule {
        priority: 35,
        kind: NixErrorKind::FlakeLockStale,
        any: &[&["git+file:"]],
        extra: Some(|t| lower_has_any(t, &["does not exist", "no such file", "error:", "failed"])),
        summary: LOCK,
    },
    Rule {
        priority: 35,
        kind: NixErrorKind::FlakeLockStale,
        any: &[&["path:"]],
        extra: Some(|t| {
            lower_has_any(t, &["does not exist", "no such file"])
                && lower_has_any(t, &["flake", "input", "lock"])
        }),
        summary: LOCK,
    },
    // flake.lock + missing path is already MissingStorePath; this catches lock wording alone.
    Rule {
        priority: 40,
        kind: NixErrorKind::FlakeLockStale,
        any: &[&["flake.lock"]],
        extra: Some(|t| {
            lower_has_any(t, &["does not exist", "no such file", "locked", "outdated"])
        }),
        summary: LOCK,
    },
    Rule {
        priority: 40,
        kind: NixErrorKind::FlakeLockStale,
        any: &[&["locked input"]],
        extra: Some(|t| lower_has_any(t, &["does not exist", "error", "failed"])),
        summary: LOCK,
    },
];
