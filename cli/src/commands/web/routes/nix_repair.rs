// Web routes for Nix store repair jobs (progress streams over /ws/op/<id>).
use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{get, post, State};

use crate::commands::web::nix_repair;
use crate::commands::web::ops::monitor::monitor_fragment;
use crate::commands::web::structs::AppConfig;
use crate::commands::web::util::{escape_html, repair_id_ok};

/// Start (or attach to) a store verify+repair job; returns the monitor fragment.
#[post("/nix/repair")]
pub fn nix_repair_start(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    let id = nix_repair::start_store_verify_repair(config.inner().clone());
    RawHtml(monitor_fragment(&id, None))
}

#[get("/nix/repair/monitor/<id>")]
pub fn nix_repair_monitor(id: &str) -> RawHtml<String> {
    if !repair_id_ok(id) {
        return RawHtml(format!(
            r#"<div class="alert alert-error text-sm">invalid repair id: {}</div>"#,
            escape_html(id)
        ));
    }
    RawHtml(monitor_fragment(id, None))
}
