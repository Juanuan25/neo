//! Activation-branch lookups and settings.toml-at-rev helpers.
use std::collections::HashMap;
use std::path::Path;

use toml_edit::DocumentMut;

use super::super::types::ServicesAtRev;
use super::plumbing::git_stdout;

/// Commit id → `activation_*` branch names pointing at it.
pub fn activation_tips(dir: &Path) -> HashMap<String, Vec<String>> {
    let mut tips: HashMap<String, Vec<String>> = HashMap::new();
    if let Ok(raw) = git_stdout(
        dir,
        &[
            "for-each-ref",
            "--format=%(objectname) %(refname:short)",
            "refs/heads/activation_*",
        ],
    ) {
        for line in raw.lines() {
            if let Some((oid, name)) = line.split_once(' ') {
                tips.entry(oid.to_string())
                    .or_default()
                    .push(name.to_string());
            }
        }
    }
    tips
}

/// Resolve a safe-looking rev to a full commit id via shell git.
pub fn resolve_rev(config_path: &str, rev: &str) -> Result<String, String> {
    let out = git_stdout(
        Path::new(config_path),
        &["rev-parse", "--verify", &format!("{}^{{commit}}", rev)],
    )?;
    let id = out.trim().to_string();
    if id.len() != 40 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("unexpected rev-parse result for {}", rev));
    }
    Ok(id)
}

/// Parse enabled/disabled services from `settings.toml` at `rev`.
pub fn enabled_services_at_rev(config_path: &str, rev: &str) -> Result<ServicesAtRev, String> {
    resolve_rev(config_path, rev)?;
    let raw = git_stdout(
        Path::new(config_path),
        &["show", &format!("{}:settings.toml", rev)],
    )?;
    let doc: DocumentMut = raw
        .parse()
        .map_err(|e| format!("parse settings.toml at {}: {e}", rev))?;
    let mut enabled = Vec::new();
    let mut disabled = Vec::new();
    if let Some(services) = doc.get("services").and_then(|i| i.as_table()) {
        for (name, item) in services.iter() {
            let is_enabled = item
                .as_table()
                .and_then(|t| t.get("enabled"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_enabled {
                enabled.push(name.to_string());
            } else {
                disabled.push(name.to_string());
            }
        }
    }
    enabled.sort();
    disabled.sort();
    Ok(ServicesAtRev {
        rev: rev.to_string(),
        enabled,
        disabled,
    })
}

/// Prefer an activation branch name that points at `rev` (full or short sha).
pub fn activation_branch_for_rev(config_path: &str, rev: &str) -> Result<Option<String>, String> {
    let full = resolve_rev(config_path, rev)?;
    Ok(activation_tips(Path::new(config_path))
        .remove(&full)
        .and_then(|names| names.into_iter().next()))
}
