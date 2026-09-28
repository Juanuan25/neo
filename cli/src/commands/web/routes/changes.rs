use std::sync::Arc;

use rocket::response::content::RawHtml;
use rocket::{get, post, State};

use crate::commands::web::action_bar::{action_bar_dynamic_element, broadcast_action_bar};
use crate::commands::web::git_ops::{dirty_state, get_settings_toml_diff};
use crate::commands::web::settings::restore_settings_from_applied;
use crate::commands::web::structs::AppConfig;
use crate::commands::web::trigger::trigger_activation;
use crate::commands::web::util::{alert_html, changes_actions_row, escape_html, AlertKind};

#[get("/changes/action-bar")]
pub fn changes_action_bar(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    RawHtml(action_bar_dynamic_element(&config, false))
}

/// Unified diff as one line per span, tinted by +/-/@@ (styles: `.neo-diff` in input.css).
fn diff_html(diff: &str) -> String {
    let mut out = String::from(
        r#"<pre class="neo-diff rounded-box border border-base-300 bg-base-200/60 py-2 overflow-auto max-h-[55vh]">"#,
    );
    for line in diff.lines() {
        let class = if line.starts_with("+++")
            || line.starts_with("---")
            || line.starts_with("diff ")
            || line.starts_with("index ")
        {
            "meta"
        } else if line.starts_with('+') {
            "add"
        } else if line.starts_with('-') {
            "del"
        } else if line.starts_with("@@") {
            "hunk"
        } else {
            ""
        };
        out.push_str(&format!(
            r#"<span class="{class}">{}</span>"#,
            escape_html(line)
        ));
    }
    out.push_str("</pre>");
    out
}

fn summary_heading(title: &str, sub: &str) -> String {
    format!(
        r#"<div class="flex items-center gap-2 mb-3"><span class="neo-unit-dot" data-state="partial" aria-hidden="true"></span><div><div class="text-sm font-semibold">{title}</div><div class="text-xs text-base-content/60">{sub}</div></div></div>"#
    )
}

#[get("/changes/summary")]
pub fn changes_summary(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    let d = dirty_state(&config);
    let body = if d.settings_dirty {
        let diff = get_settings_toml_diff(&config);
        format!(
            "{}{}{}",
            summary_heading(
                "settings.toml",
                "Saved but not yet activated. Activate to apply, or discard to return to the last applied state."
            ),
            diff_html(&diff),
            changes_actions_row()
        )
    } else if d.worktree_dirty {
        let esc = escape_html(&d.summary);
        format!(
            "{}<pre class=\"text-xs overflow-auto max-h-[55vh] rounded-box border border-base-300 bg-base-200/60 p-3 whitespace-pre\">{}</pre>{}",
            summary_heading("Working tree", "Other files changed since the last activation."),
            esc,
            changes_actions_row()
        )
    } else {
        r#"<div class="flex flex-col items-center text-center gap-2 py-8"><span class="w-10 h-10 rounded-full bg-success/15 text-success flex items-center justify-center"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" class="w-5 h-5" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg></span><div class="font-semibold text-sm">Everything is applied</div><div class="text-xs text-base-content/60">No pending changes in the working tree.</div></div>"#.to_string()
    };
    RawHtml(body)
}

#[post("/changes/revert")]
pub fn revert_settings(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    match restore_settings_from_applied(&config) {
        Ok(()) => RawHtml(alert_html(
            AlertKind::Success,
            "Reverted via paste-settings. Close and reload options to see state.",
        )),
        Err(e) => RawHtml(alert_html(
            AlertKind::Error,
            &format!("Revert failed: {}", e),
        )),
    }
}

/// Shared with `/actions/activate`: trigger activation oneshot and refresh action bar.
pub fn apply_or_activate(config: &AppConfig) -> RawHtml<String> {
    let html = trigger_activation(config);
    broadcast_action_bar(config);
    html
}

#[post("/changes/apply")]
pub fn apply_settings(config: &State<Arc<AppConfig>>) -> RawHtml<String> {
    apply_or_activate(&config)
}
