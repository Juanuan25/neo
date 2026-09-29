//! Monitor mount point for background ops; progress streams over `/ws/op/<id>`.
use rocket::get;
use rocket::response::content::RawHtml;

use crate::commands::web::ops::monitor::monitor_fragment;

/// Any op kind (action-bar progress button, resume after a page reload).
#[get("/op/monitor/<id>")]
pub fn op_monitor(id: &str) -> RawHtml<String> {
    RawHtml(monitor_fragment(id, None))
}
