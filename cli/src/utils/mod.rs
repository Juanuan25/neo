//! Shared helpers used by CLI commands and the web UI.
//!
//! Keep `crate::commands` for subcommand entry points only.

pub mod command;
pub mod generation;
pub mod ops;
pub mod profile;
pub mod settings;
pub mod stdio_tee;
pub mod toml_sort;

pub use command::{
    execute_command, format_command, get_current_branch, get_timestamp, git_cmd,
    has_staged_changes, run_nix, run_write_flake, shell_join, shell_quote,
};
pub use generation::{
    activation_commit_message, current_generation_number, list_system_generations,
    list_system_generations_with_sudo, parse_generation_from_message, record_generation_in_commit,
    running_generation_number, switch_system_generation, system_profile_available, GenerationMode,
    GenerationTimeline, GenerationsList, SystemGeneration,
};
pub use ops::{operations_dir, resolve_suffix, OperationKind, OperationLog, OPERATIONS_DIR};
pub use profile::{
    is_local_flake_ref, local_flake_dir, neo_cli_get, normalize_profile_arg, resolve_config_path,
    resolve_neo_input, resolve_profile, resolve_template, set_profile_str, template_from_neo_input,
    DEFAULT_NEO_INPUT, DEFAULT_TEMPLATE, PROFILE_LOCAL, PROFILE_SERVER,
};
pub use settings::load_or_default_settings;
pub use toml_sort::sort_document_alphabetically;
