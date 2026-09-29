use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{post, State};

use crate::commands::web::action_bar::broadcast_action_bar;
use crate::commands::web::locks::Blocked;
use crate::commands::web::routes::changes::{apply_or_activate, discard_response};
use crate::commands::web::trigger::trigger_update;
use crate::commands::web::types::AppConfig;

#[post("/flake/update")]
pub fn flake_update(config: &State<Arc<AppConfig>>) -> Result<RawHtml<String>, Blocked> {
    let html = trigger_update()?;
    broadcast_action_bar(config);
    Ok(html)
}

#[post("/actions/activate")]
pub fn actions_activate(config: &State<Arc<AppConfig>>) -> Result<RawHtml<String>, Blocked> {
    apply_or_activate(config)
}

#[post("/actions/reset")]
pub fn actions_reset(config: &State<Arc<AppConfig>>) -> Result<RawHtml<String>, Blocked> {
    discard_response(config)
}
