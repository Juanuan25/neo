//! Live operation-lock state (`#neo-locks`); later changes arrive over `/ws/status`.
use rocket::get;
use rocket::response::content::RawHtml;

use crate::commands::web::locks::locks_element;

#[get("/locks")]
pub async fn locks_state() -> RawHtml<String> {
    RawHtml(locks_element().await)
}
