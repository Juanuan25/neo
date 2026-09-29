//! JSON endpoint backing the services-overview resources panel
//! (`cli/static/resources_panel.js`). See `commands/web/resources` for the
//! `/proc` sampling this just serializes.
use std::sync::Arc;

use rocket::serde::json::Json;
use rocket::{get, State};

use crate::commands::web::resources::Snapshot;
use crate::commands::web::types::AppConfig;

/// Current system resource snapshot (CPU/mem/disks/net/load/uptime/top
/// processes). Polled by the panel at ~2-5s while visible; the sampler
/// throttles actual `/proc` reads independently of request rate.
#[get("/resources/snapshot")]
pub async fn resources_snapshot(config: &State<Arc<AppConfig>>) -> Json<Snapshot> {
    Json(config.resources.snapshot().await)
}
