//! Systemd unit control: active state, start/stop/restart, control OOB fragments.
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::Command as AsyncCommand;

use super::super::types::AppConfig;
use super::super::util::{escape_attr, escape_html, status_slot_oob, sudo_cmd};

pub use super::super::util::unit_name_valid;

/// systemctl action allowed from the web UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitAction {
    Start,
    Stop,
    Restart,
}

impl UnitAction {
    pub fn as_str(self) -> &'static str {
        match self {
            UnitAction::Start => "start",
            UnitAction::Stop => "stop",
            UnitAction::Restart => "restart",
        }
    }
}

pub fn is_pull_in_flight(config: &AppConfig, unit: &str) -> bool {
    config.pulls_in_flight.contains(unit)
}

/// Mark unit as pulling. Returns false if a pull is already in flight for this unit.
pub fn try_begin_pull(config: &AppConfig, unit: &str) -> bool {
    config.pulls_in_flight.try_begin(unit)
}

pub fn end_pull(config: &AppConfig, unit: &str) {
    config.pulls_in_flight.end(unit);
}

/// Normalize systemctl is-active stdout into a state string.
fn parse_active_state_stdout(stdout: &[u8]) -> String {
    let s = String::from_utf8_lossy(stdout).trim().to_string();
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

/// Query systemctl is-active for a unit (sync; used when building OOB control fragments).
fn unit_active_state(unit: &str) -> String {
    let sudo = sudo_cmd();
    Command::new(&sudo)
        .args(["systemctl", "is-active", unit])
        .output()
        .map(|o| parse_active_state_stdout(&o.stdout))
        .unwrap_or_else(|_| "unknown".into())
}

pub async fn unit_active_state_async(unit: &str) -> String {
    let sudo = sudo_cmd();
    match AsyncCommand::new(&sudo)
        .args(["systemctl", "is-active", unit])
        .output()
        .await
    {
        Ok(o) => parse_active_state_stdout(&o.stdout),
        Err(_) => "unknown".into(),
    }
}

const UPDATE_OUT_CLASSES: &str =
    "update-out update-out-inline text-[10px] max-w-full truncate empty:hidden";

/// OOB fragment for the per-row pull status slot (`#update-out-{unit}`).
pub fn update_out_oob(unit: &str, inner: &str, title: &str) -> String {
    status_slot_oob("update-out", unit, UPDATE_OUT_CLASSES, inner, title)
}

pub fn broadcast_update_out(unit: &str, inner: &str, title: &str, config: &AppConfig) {
    let _ = config.unit_updates.send(update_out_oob(unit, inner, title));
}

/// Inline icons for the unit control buttons (currentColor, 16px).
const ICON_START: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="w-4 h-4" aria-hidden="true"><path d="M6.3 3.9A1 1 0 004.8 4.8v10.4a1 1 0 001.5.9l8.6-5.2a1 1 0 000-1.8L6.3 3.9z"/></svg>"#;
const ICON_STOP: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="w-4 h-4" aria-hidden="true"><rect x="5" y="5" width="10" height="10" rx="1.5"/></svg>"#;
const ICON_RESTART: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4" aria-hidden="true"><path d="M19.5 12a7.5 7.5 0 11-2.2-5.3M19.5 4.5v4h-4"/></svg>"#;
const ICON_PULL: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4" aria-hidden="true"><path d="M12 4v11M7.5 10.5L12 15l4.5-4.5M5 19.5h14"/></svg>"#;
const ICON_LOGS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="w-4 h-4" aria-hidden="true"><path d="M4.5 6.5l4 4-4 4M11 17.5h8.5"/></svg>"#;

const BTN: &str = "btn btn-ghost btn-sm btn-square";

/// Map a raw systemd ActiveState onto a `.neo-unit-dot` summary state.
fn dot_state(active: &str) -> &'static str {
    match active {
        "active" => "ok",
        "inactive" => "none",
        "activating" | "deactivating" | "reloading" | "refreshing" => "changing",
        "failed" => "failed",
        _ => "partial",
    }
}

/// Build the inner content (state + buttons) for a unit controls area.
/// Used for OOB WS pushes and composed into full divs.
///
/// Layout is a fixed set of slots so every row in the Status tab lines up
/// regardless of state or unit type: `[state] [start|stop] [restart] [pull|·] [logs]`.
/// Non-container units get an empty placeholder in the pull slot.
///
/// Buttons stay stable across transitional states so restart/stop never "vanish"
/// while systemctl --no-block is still settling (the live WS watcher re-renders
/// as soon as ActiveState changes).
fn render_unit_controls_content_with_state(unit: &str, active: &str, pulling: bool) -> String {
    let is_container = unit.starts_with("docker-");

    let u = escape_html(unit);
    // Basic JS string escape for onclick arg (single quotes in unit names are rare for units)
    let u_js = u.replace('\'', "\\'");

    let mut inner = format!(
        r#"<span class="neo-unit-state" title="ActiveState of {title}"><span class="neo-unit-dot" data-state="{dot}" aria-hidden="true"></span><span class="truncate">{label}</span></span>"#,
        title = escape_attr(unit),
        dot = dot_state(active),
        label = escape_html(active),
    );

    // Primary slot: inactive/failed → start; anything running/transitional → stop.
    let primary = match active {
        "inactive" | "failed" => format!(
            r##"<button type="button" class="{BTN} text-success" hx-post="/unit/start/{u}" hx-swap="none" title="Start (systemctl start)" aria-label="Start {u}">{ICON_START}</button>"##
        ),
        _ => format!(
            r##"<button type="button" class="{BTN} text-error" hx-post="/unit/stop/{u}" hx-swap="none" title="Stop (systemctl stop)" aria-label="Stop {u}">{ICON_STOP}</button>"##
        ),
    };
    inner.push_str(&primary);
    inner.push_str(&format!(
        r##"<button type="button" class="{BTN}" hx-post="/unit/restart/{u}" hx-swap="none" title="Restart (systemctl restart)" aria-label="Restart {u}">{ICON_RESTART}</button>"##
    ));

    if !is_container {
        inner.push_str(r#"<span class="neo-unit-slot" aria-hidden="true"></span>"#);
    } else if pulling {
        inner.push_str(&format!(
            r#"<button type="button" class="{BTN} btn-disabled" disabled title="docker pull in progress"><span class="loading loading-spinner loading-xs"></span></button>"#
        ));
    } else {
        // hx-swap=none: immediate OOB (update-out + controls) comes from the response;
        // long pull progress is pushed over /ws/status.
        inner.push_str(&format!(
            r##"<button type="button" class="{BTN}" hx-post="/container/update/{u}" hx-swap="none" hx-disabled-elt="this" title="Pull the image and restart (docker pull + restart)" aria-label="Update image of {u}">{ICON_PULL}</button>"##
        ));
    }

    // Logs always opens the dialog (live via SSE).
    inner.push_str(&format!(
        r#"<button type="button" class="{BTN}" onclick="openUnitLogs('{u_js}')" title="Live logs" aria-label="Logs of {u}">{ICON_LOGS}</button>"#
    ));

    inner
}

/// OOB fragment for htmx ws (and action HTTP responses).
pub fn unit_controls_oob_fragment(unit: &str, config: &AppConfig) -> String {
    let active = unit_active_state(unit);
    let pulling = is_pull_in_flight(config, unit);
    unit_controls_oob_fragment_with_state(unit, &active, pulling)
}

pub fn unit_controls_oob_fragment_with_state(unit: &str, active: &str, pulling: bool) -> String {
    format!(
        r#"<div id="unit-controls-{}" class="unit-controls" data-active-state="{}" hx-swap-oob="true">{}</div>"#,
        escape_html(unit),
        escape_attr(active),
        render_unit_controls_content_with_state(unit, active, pulling)
    )
}

/// Broadcast an OOB swap fragment for a unit's controls to all connected WS clients.
pub fn broadcast_unit_update(unit: &str, config: &AppConfig) {
    let _ = config
        .unit_updates
        .send(unit_controls_oob_fragment(unit, config));
}

/// After a non-blocking systemctl action, ActiveState may lag for a few seconds.
/// Push a short burst of refreshes so the UI settles without waiting for the next
/// watcher tick alone (and even if the pane only did a one-shot HTTP OOB).
pub fn schedule_unit_refresh_burst(unit: String, config: Arc<AppConfig>) {
    if !unit_name_valid(&unit) {
        return;
    }
    tokio::spawn(async move {
        for delay_ms in [150_u64, 400, 900, 1800, 3500] {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            broadcast_unit_update(&unit, &config);
        }
    });
}

pub fn perform_unit_action(action: UnitAction, unit: &str) {
    if !unit_name_valid(unit) {
        return;
    }
    let sudo = sudo_cmd();
    let _ = Command::new(&sudo)
        .args([
            "systemctl",
            action.as_str(),
            unit,
            "--no-block",
            "--no-ask-password",
        ])
        .status();
}

/// Best-effort parse of `id="unit-controls-…"` + `data-active-state="…"` from an OOB fragment.
pub fn extract_unit_state_from_oob(fragment: &str) -> Option<(String, String)> {
    let id_marker = r#"id="unit-controls-"#;
    let state_marker = r#"data-active-state=""#;
    let id_start = fragment.find(id_marker)? + id_marker.len();
    let id_end = fragment[id_start..].find('"')? + id_start;
    let unit = fragment[id_start..id_end].to_string();
    let state_start = fragment.find(state_marker)? + state_marker.len();
    let state_end = fragment[state_start..].find('"')? + state_start;
    let state = fragment[state_start..state_end].to_string();
    if unit_name_valid(&unit) {
        Some((unit, state))
    } else {
        None
    }
}

/// Normalize path param to (systemd unit name, bare docker container name).
pub fn normalize_container_unit(container: &str) -> (String, String) {
    match container.strip_prefix("docker-") {
        Some(bare) => (container.to_string(), bare.to_string()),
        None => (format!("docker-{container}"), container.to_string()),
    }
}
