//! Activation / update / generation-switch lookups and monitor mount points.
//! Live progress is streamed by `/ws/op/<id>` (see [`super::ops::monitor`]).
use crate::commands::web::ops::monitor::monitor_fragment;
use crate::commands::web::ops::store::{find_recent_in_progress, gc_old_ops};

pub fn find_recent_in_progress_activation() -> Option<String> {
    find_recent_in_progress("activation_")
}

pub fn find_recent_in_progress_update() -> Option<String> {
    find_recent_in_progress("update_")
}

pub fn find_recent_in_progress_genswitch() -> Option<String> {
    find_recent_in_progress("genswitch_")
}

pub fn gc_old_activations() {
    gc_old_ops()
}

/// Monitor for any op id (activation / update / repair / genswitch).
pub fn build_monitor_fragment(id: &str) -> String {
    monitor_fragment(id, None)
}

/// Monitor for a detached generation switch/boot: neo-web is restarted by the switch.
pub fn build_genswitch_monitor_fragment(id: &str, generation: u64, mode: &str) -> String {
    let what = if mode == "boot" {
        format!("Setting generation {generation} as the boot default.")
    } else {
        format!("Switching to generation {generation}.")
    };
    monitor_fragment(
        id,
        Some(&format!(
            "{what} The web UI may restart during the switch; the output reconnects automatically."
        )),
    )
}

pub fn is_activation_in_progress() -> bool {
    find_recent_in_progress_activation().is_some() || find_recent_in_progress_genswitch().is_some()
}
