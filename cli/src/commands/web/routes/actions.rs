use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{post, State};

use crate::commands::web::action_bar::broadcast_action_bar;
use crate::commands::web::routes::changes::apply_or_activate;
use crate::commands::web::settings::discard_pending_changes;
use crate::commands::web::structs::AppConfig;
use crate::commands::web::trigger::trigger_update;
use crate::commands::web::util::{alert_html, AlertKind};

#[post("/flake/update")]
pub fn flake_update(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    let html = trigger_update();
    broadcast_action_bar(&config);
    html
}

#[post("/actions/activate")]
pub fn actions_activate(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    apply_or_activate(&config)
}

#[post("/actions/reset")]
pub fn actions_reset(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    match discard_pending_changes(&config) {
        Ok(()) => RawHtml(alert_html(
            AlertKind::Success,
            "Pending changes discarded — back to the last committed configuration.",
        )),
        Err(e) => RawHtml(alert_html(
            AlertKind::Error,
            &format!("Discard failed: {}", e),
        )),
    }
}
