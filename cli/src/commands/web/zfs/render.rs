//! Shared snapshot list markup for the option pane ([`super::service`]) and the
//! versioning tab ([`super::data`]), so both lists look and behave the same.
use super::super::util::{escape_attr, escape_html};
use super::{format_age, format_epoch_utc, human_bytes, snapshot_kind, Snapshot};

/// Chevron used by the collapsible `<details>` summaries.
pub const CHEVRON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20" fill="currentColor" class="w-3.5 h-3.5 shrink-0 text-base-content/50 transition-transform group-open:rotate-90" aria-hidden="true"><path fill-rule="evenodd" d="M7.2 14.8a.75.75 0 010-1.06L10.94 10 7.2 6.26a.75.75 0 111.06-1.06l4.27 4.27a.75.75 0 010 1.06L8.26 14.8a.75.75 0 01-1.06 0z" clip-rule="evenodd"/></svg>"#;

const CAMERA: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" class="w-4 h-4" aria-hidden="true"><path d="M4 8.5A1.5 1.5 0 015.5 7h2.3l1.5-2h5.4l1.5 2h2.3A1.5 1.5 0 0120 8.5v9a1.5 1.5 0 01-1.5 1.5h-13A1.5 1.5 0 014 17.5v-9z"/><circle cx="12" cy="13" r="3.2"/></svg>"#;

/// Button classes for a secondary (non-destructive) row / header action.
pub const BTN_SECONDARY: &str = "btn btn-sm btn-ghost bg-base-100 border-base-300";

/// One snapshot row: icon, time + kind badge, `age · size{meta}` and actions.
/// `meta_html` is appended to the second line (already escaped HTML).
/// `highlight` tints the icon (snapshots this scope created).
pub fn snapshot_row(
    s: &Snapshot,
    now: i64,
    meta_html: &str,
    actions_html: &str,
    highlight: bool,
) -> String {
    let (kind, badge) = snapshot_kind(&s.name);
    let icon_cls = if highlight {
        "bg-info/15 text-info"
    } else {
        "bg-base-200 text-base-content/60"
    };
    format!(
        r#"<li class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-3 px-3 sm:px-4 py-2.5">
<div class="flex items-center gap-3 min-w-0 flex-1">
<span class="w-9 h-9 shrink-0 rounded-xl {icon_cls} flex items-center justify-center">{CAMERA}</span>
<div class="min-w-0"><div class="flex items-center gap-1.5 flex-wrap"><span class="text-sm font-semibold tabular-nums" title="{full}">{when}</span><span class="badge badge-xs {badge}">{kind}</span></div>
<div class="text-xs text-base-content/55 truncate">{age} · <span title="Space held only by this snapshot">{used}</span>{meta_html}</div></div>
</div>
<div class="flex flex-wrap gap-1.5 sm:justify-end pl-12 sm:pl-0">{actions_html}</div></li>"#,
        kind = escape_html(&kind),
        full = escape_attr(&s.full),
        when = escape_html(&format_epoch_utc(s.creation)),
        age = format_age(now, s.creation),
        used = human_bytes(s.used),
    )
}

/// Collapsible list card: `<details>` with title, count and dataset, then rows
/// (newest first, at most `max_rows`) or an empty state.
pub fn snapshot_list(
    title: &str,
    ds: &str,
    open: bool,
    total: usize,
    rows: &[String],
    max_rows: usize,
) -> String {
    let mut html = format!(
        r#"<details class="group rounded-box border border-base-300 bg-base-100 overflow-hidden"{open}>
<summary class="cursor-pointer list-none px-3 sm:px-4 py-2.5 flex items-center gap-2 border-b border-transparent group-open:border-base-300">{CHEVRON}<span class="text-sm font-semibold">{title}</span><span class="text-xs text-base-content/40 tabular-nums">{total}</span><span class="font-mono text-[10px] text-base-content/40 ml-auto truncate hidden sm:inline" title="ZFS dataset">{ds}</span></summary>"#,
        open = if open { " open" } else { "" },
        title = escape_html(title),
        ds = escape_html(ds),
    );
    if rows.is_empty() {
        html.push_str(
            r#"<div class="px-4 py-6 text-center text-xs text-base-content/50">No snapshots yet.</div>"#,
        );
    } else {
        html.push_str(r#"<ul class="divide-y divide-base-300/70 max-h-[28rem] overflow-auto">"#);
        for r in rows {
            html.push_str(r);
        }
        html.push_str("</ul>");
        if total > max_rows {
            html.push_str(&format!(
                r#"<div class="px-4 py-2 text-[11px] text-base-content/50 border-t border-base-300">{max_rows} newest of {total} shown.</div>"#
            ));
        }
    }
    html.push_str("</details>");
    html
}

/// "How restoring works" disclosure; `items_html` are `<li>` contents.
pub fn how_it_works(items_html: &[&str]) -> String {
    let items: String = items_html.iter().map(|i| format!("<li>{i}</li>")).collect();
    format!(
        r#"<details class="group rounded-box border border-base-300 bg-base-100 text-xs text-base-content/70">
<summary class="cursor-pointer list-none px-3 sm:px-4 py-2.5 flex items-center gap-2 font-medium text-base-content/80">{CHEVRON}How restoring works</summary>
<ul class="px-3 sm:px-4 pb-3 pl-8 sm:pl-9 space-y-1 list-disc leading-relaxed">{items}</ul>
</details>"#
    )
}
