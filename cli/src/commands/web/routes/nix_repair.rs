// Web routes for Nix store repair jobs (progress streams over /ws/op/<id>).
use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{post, State};

use crate::commands::web::locks::Blocked;
use crate::commands::web::nix_repair;
use crate::commands::web::ops::monitor::monitor_fragment;
use crate::commands::web::types::AppConfig;

/// Start (or attach to) a store verify+repair job; returns the monitor fragment.
#[post("/nix/repair")]
pub fn nix_repair_start(config: &State<Arc<AppConfig>>) -> Result<RawHtml<String>, Blocked> {
    let id = nix_repair::start_store_verify_repair(config.inner().clone())?;
    Ok(RawHtml(monitor_fragment(&id, None)))
}
