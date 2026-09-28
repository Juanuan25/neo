use super::escape_html;

/// Unified diff as one line per span, tinted by +/-/@@ (styles: `.neo-diff` in input.css).
pub fn diff_html(diff: &str) -> String {
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
