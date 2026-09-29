//! Monitor mount points for background ops; progress streams over `/ws/op/<id>`.
use rocket::get;
use rocket::response::content::RawHtml;

use crate::commands::web::ops::monitor::monitor_fragment;

#[get("/activation/monitor/<id>")]
pub fn activation_monitor(id: &str) -> RawHtml<String> {
    RawHtml(monitor_fragment(id, None))
}

#[get("/update/monitor/<id>")]
pub fn update_monitor(id: &str) -> RawHtml<String> {
    RawHtml(monitor_fragment(id, None))
}

/// Any op kind (used to resume a monitor after a page reload).
#[get("/op/monitor/<id>")]
pub fn op_monitor(id: &str) -> RawHtml<String> {
    RawHtml(monitor_fragment(id, None))
}
