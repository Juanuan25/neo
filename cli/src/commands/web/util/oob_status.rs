//! Status chips (`(inner_html, title)`) for OOB status slots (pulls, clear-appdata, snapshots).
use super::escape::{escape_attr, escape_html};

/// Spinner + truncated message (in-progress).
pub fn status_pulling(msg: &str) -> (String, String) {
    let inner = format!(
        r#"<span class="inline-flex items-center gap-1 text-info max-w-full"><span class="loading loading-spinner loading-xs flex-shrink-0"></span><span class="truncate">{}</span></span>"#,
        escape_html(msg)
    );
    (inner, msg.to_string())
}

fn status_mark(class: &str, mark: &str, msg: &str) -> (String, String) {
    let inner = format!(
        r#"<span class="{class} truncate">{mark} {}</span>"#,
        escape_html(msg)
    );
    (inner, msg.to_string())
}

/// Success checkmark + message.
pub fn status_ok(msg: &str) -> (String, String) {
    status_mark("text-success", "✓", msg)
}

/// Error mark + message.
pub fn status_err(msg: &str) -> (String, String) {
    status_mark("text-error", "✗", msg)
}

/// Generic OOB status slot: `<div id="{prefix}-{key}" … hx-swap-oob>`.
pub fn status_slot_oob(prefix: &str, key: &str, classes: &str, inner: &str, title: &str) -> String {
    format!(
        r#"<div id="{prefix}-{key}" class="{classes}" title="{title}" hx-swap-oob="true">{inner}</div>"#,
        prefix = escape_html(prefix),
        key = escape_html(key),
        title = escape_attr(title),
    )
}
