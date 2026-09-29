mod actions;
mod activation;
mod branches;
mod changes;
mod helpers;
mod nix_repair;
mod oauth;
mod pages;
mod save;
mod settings_file;
mod snapshots;
mod ssh;
mod units;
mod ws;

use rocket::routes;

pub fn routes() -> Vec<rocket::Route> {
    routes![
        pages::site_webmanifest,
        pages::index,
        pages::nav_services,
        // Canonical configuration URLs (more specific paths before legacy aliases).
        pages::configuration,
        pages::configuration_services,
        pages::configuration_settings,
        pages::configuration_versioning,
        pages::configuration_option,
        pages::configuration_core,
        // Legacy partial aliases (still used as fallbacks; non-HTMX redirects).
        pages::option_pane,
        pages::services_grid,
        pages::core_grid,
        pages::core_pane,
        helpers::run_helper,
        oauth::run_oauth,
        save::save_service,
        save::save_core_section,
        changes::changes_action_bar,
        changes::changes_summary,
        changes::revert_settings,
        changes::apply_settings,
        actions::flake_update,
        actions::actions_activate,
        actions::actions_reset,
        branches::branches,
        branches::git_switch,
        branches::versioning_graph,
        branches::versioning_tree,
        branches::versioning_services,
        branches::versioning_diff,
        branches::versioning_generations,
        branches::versioning_activate,
        branches::versioning_gen_switch,
        activation::activation_monitor,
        activation::update_monitor,
        activation::op_monitor,
        nix_repair::nix_repair_start,
        nix_repair::nix_repair_monitor,
        units::unit_restart,
        units::unit_start,
        units::unit_stop,
        units::container_update,
        units::clear_appdata,
        units::services_status,
        snapshots::service_snapshots,
        snapshots::service_snapshot_create,
        snapshots::service_snapshot_restore,
        snapshots::versioning_zfs,
        snapshots::versioning_zfs_snapshot,
        snapshots::versioning_zfs_restore,
        snapshots::versioning_zfs_reboot,
        snapshots::versioning_zfs_cancel,
        snapshots::versioning_zfs_dismiss,
        units::sse_logs,
        ws::ws_status,
        ws::ws_op,
        ssh::ssh_public_key_card,
        ssh::ssh_regenerate,
        settings_file::download_settings,
        settings_file::upload_settings,
    ]
}
