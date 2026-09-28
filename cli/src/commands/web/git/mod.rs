//! Git plumbing and versioning helpers for the web UI.
mod changes;
mod dirty;
mod plumbing;
pub(crate) use plumbing::git_stdout;
mod versioning;

pub use changes::{collect_changes, settings_semantic, DiffRange};
pub use dirty::{dirty_state, is_worktree_dirty, DirtyState};
pub use versioning::{
    activation_branch_for_rev, activation_graph, enabled_services_at_rev, list_activation_branches,
    resolve_rev, settings_at_rev,
};
