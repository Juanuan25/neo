// Classify raw Nix evaluator / stderr text into stable error kinds for the UI.
mod patterns;
mod remediate;

pub use patterns::classify;
use remediate::{offers_flake_update, offers_store_repair, plan_for};

use crate::commands::web::types::EvalErrorUi;

/// Stable categories operators and remediation code can switch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NixErrorKind {
    MissingStorePath,
    FlakeLockStale,
    EvalAssertion,
    InfiniteRecursion,
    UndefinedVariable,
    HashMismatch,
    NetworkFetchFailed,
    PermissionDenied,
    Timeout,
    ProcessDied,
    Unknown,
}

impl NixErrorKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::MissingStorePath => "missing-store-path",
            Self::FlakeLockStale => "flake-lock-stale",
            Self::EvalAssertion => "eval-assertion",
            Self::InfiniteRecursion => "infinite-recursion",
            Self::UndefinedVariable => "undefined-variable",
            Self::HashMismatch => "hash-mismatch",
            Self::NetworkFetchFailed => "network-fetch-failed",
            Self::PermissionDenied => "permission-denied",
            Self::Timeout => "timeout",
            Self::ProcessDied => "process-died",
            Self::Unknown => "unknown",
        }
    }
}

/// Structured Nix failure for banners, logs, and future remediation.
#[derive(Debug, Clone)]
pub struct NixError {
    pub kind: NixErrorKind,
    /// One-line operator-facing summary.
    pub summary: String,
    /// Truncated multi-line detail (stderr / anyhow chain).
    pub detail: String,
    /// Store paths extracted from the message when present.
    pub paths: Vec<String>,
}

impl NixError {
    /// Compact message for existing `error: Option<String>` template fields.
    pub fn user_message(&self) -> String {
        let mut msg = format!("[{}] {}", self.kind.id(), self.summary);
        if let Some(p) = self.paths.first() {
            if !self.summary.contains(p) {
                msg.push_str(&format!(" ({p})"));
            }
        }
        msg
    }

    /// Longer message for panes / banners (summary + short detail tail).
    pub fn display_message(&self) -> String {
        let detail = self.detail.trim();
        if detail.is_empty() || detail == self.summary {
            return self.user_message();
        }
        let tail = if detail.len() > 800 {
            // Prefer leaf error at the end of Nix traces.
            let start = detail
                .char_indices()
                .rev()
                .nth(799)
                .map(|(i, _)| i)
                .unwrap_or(0);
            format!("…{}", &detail[start..])
        } else {
            detail.to_string()
        };
        format!("{}\n{}", self.user_message(), tail)
    }
}

/// UI banner for a failed extract: `context`, classified summary, remediation hint
/// and which repair actions to offer.
pub fn eval_error_ui(context: &str, err: &anyhow::Error) -> EvalErrorUi {
    let nix_err = classify(&format!("{err:#}"));
    let help = plan_for(nix_err.kind).help;
    EvalErrorUi {
        error: Some(format!("{context}: {}. {help}", nix_err.display_message())),
        error_kind: Some(nix_err.kind.id().to_string()),
        can_store_repair: offers_store_repair(nix_err.kind),
        can_flake_update: offers_flake_update(nix_err.kind),
    }
}
