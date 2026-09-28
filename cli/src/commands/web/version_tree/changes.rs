//! One-line "what changed" per config commit: services enabled / disabled /
//! reconfigured and other top-level sections touched, from `settings.toml` at
//! the commit and at its first parent. Blobs are read in one
//! `git cat-file --batch` call for the whole window.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;
use toml_edit::{DocumentMut, Item};

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSummary {
    /// First version (no parent to compare with).
    pub initial: bool,
    /// settings.toml identical to the parent (re-activation, other files).
    pub unchanged: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    /// Enabled services whose settings changed.
    pub changed: Vec<String>,
    /// Other top-level sections that changed (e.g. `core`, `swag`).
    pub sections: Vec<String>,
    /// Services enabled in this version.
    pub enabled: usize,
}

struct Parsed {
    services: BTreeMap<String, (bool, String)>,
    sections: BTreeMap<String, String>,
}

fn parse(raw: &str) -> Option<Parsed> {
    let doc: DocumentMut = raw.parse().ok()?;
    let mut services = BTreeMap::new();
    let mut sections = BTreeMap::new();
    for (k, item) in doc.iter() {
        if k == "services" {
            if let Some(t) = item.as_table_like() {
                for (name, svc) in t.iter() {
                    let enabled = svc
                        .as_table_like()
                        .and_then(|t| t.get("enabled"))
                        .and_then(Item::as_bool)
                        .unwrap_or(false);
                    let mut body = svc.clone();
                    if let Some(t) = body.as_table_like_mut() {
                        t.remove("enabled");
                    }
                    services.insert(name.to_string(), (enabled, normalize(&body)));
                }
            }
        } else {
            sections.insert(k.to_string(), normalize(item));
        }
    }
    Some(Parsed { services, sections })
}

/// Formatting-insensitive rendering of an item (comments and whitespace are
/// not changes).
fn normalize(item: &Item) -> String {
    match item {
        Item::None => String::new(),
        Item::Value(v) => v.to_string().trim().to_string(),
        Item::Table(t) => {
            let mut parts: Vec<String> = t
                .iter()
                .map(|(k, v)| format!("{k}={}", normalize(v)))
                .collect();
            parts.sort();
            format!("{{{}}}", parts.join(","))
        }
        Item::ArrayOfTables(a) => {
            let parts: Vec<String> = a
                .iter()
                .map(|t| normalize(&Item::Table(t.clone())))
                .collect();
            format!("[{}]", parts.join(","))
        }
    }
}

/// Compare settings at a commit (`new`) with its parent (`old`).
pub fn summarize(old: Option<&str>, new: &str) -> Option<ChangeSummary> {
    let n = parse(new)?;
    let enabled = n.services.values().filter(|(e, _)| *e).count();
    let Some(old) = old else {
        return Some(ChangeSummary {
            initial: true,
            enabled,
            ..Default::default()
        });
    };
    if old == new {
        return Some(ChangeSummary {
            unchanged: true,
            enabled,
            ..Default::default()
        });
    }
    let o = parse(old)?;
    let mut s = ChangeSummary {
        enabled,
        ..Default::default()
    };
    let names: BTreeSet<&String> = n.services.keys().chain(o.services.keys()).collect();
    for name in names {
        let a = o.services.get(name);
        let b = n.services.get(name);
        let was = a.is_some_and(|(e, _)| *e);
        let is = b.is_some_and(|(e, _)| *e);
        match (was, is) {
            (false, true) => s.added.push(name.clone()),
            (true, false) => s.removed.push(name.clone()),
            (true, true) if a.map(|x| &x.1) != b.map(|x| &x.1) => s.changed.push(name.clone()),
            _ => {}
        }
    }
    let keys: BTreeSet<&String> = n.sections.keys().chain(o.sections.keys()).collect();
    for k in keys {
        if n.sections.get(k) != o.sections.get(k) {
            s.sections.push(k.clone());
        }
    }
    s.unchanged =
        s.added.is_empty() && s.removed.is_empty() && s.changed.is_empty() && s.sections.is_empty();
    Some(s)
}

/// `settings.toml` contents at each rev (missing file / rev → absent).
fn read_blobs(dir: &Path, revs: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if revs.is_empty() {
        return out;
    }
    let Ok(mut child) = Command::new("git")
        .current_dir(dir)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return out;
    };
    let input: String = revs
        .iter()
        .map(|r| format!("{r}:settings.toml\n"))
        .collect();
    // Write from a thread so a full stdout pipe cannot deadlock us.
    let writer = child.stdin.take().map(|mut stdin| {
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        })
    });
    if let Some(stdout) = child.stdout.take() {
        let mut r = BufReader::new(stdout);
        for rev in revs {
            let mut header = String::new();
            if r.read_line(&mut header).unwrap_or(0) == 0 {
                break;
            }
            // `<oid> blob <size>` or `<input> missing`
            let mut parts = header.split_whitespace();
            let (_, kind, size) = (parts.next(), parts.next(), parts.next());
            let Some(size) = size.and_then(|s| s.parse::<usize>().ok()) else {
                continue;
            };
            let mut buf = vec![0u8; size + 1]; // content + trailing LF
            if r.read_exact(&mut buf).is_err() {
                break;
            }
            if kind == Some("blob") {
                buf.truncate(size);
                out.insert(rev.clone(), String::from_utf8_lossy(&buf).into_owned());
            }
        }
    }
    if let Some(w) = writer {
        let _ = w.join();
    }
    let _ = child.wait();
    out
}

/// Summaries for `(commit, first parent)` pairs, keyed by commit.
pub fn summarize_commits(
    dir: &Path,
    commits: &[(String, Option<String>)],
) -> HashMap<String, ChangeSummary> {
    let mut revs: Vec<String> = Vec::new();
    for (c, p) in commits {
        revs.push(c.clone());
        if let Some(p) = p {
            revs.push(p.clone());
        }
    }
    revs.sort();
    revs.dedup();
    let blobs = read_blobs(dir, &revs);
    commits
        .iter()
        .filter_map(|(c, p)| {
            let new = blobs.get(c)?;
            let old = match p {
                Some(p) => Some(blobs.get(p).map(String::as_str).unwrap_or("")),
                None => None,
            };
            summarize(old, new).map(|s| (c.clone(), s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
[core]
timezone = "UTC"

[services.jellyfin]
enabled = true
subdomain = "tv"

[services.immich]
enabled = true

[services.paperless]
enabled = false
"#;

    #[test]
    fn initial_and_unchanged() {
        let s = summarize(None, BASE).unwrap();
        assert!(s.initial);
        assert_eq!(s.enabled, 2);
        let s = summarize(Some(BASE), BASE).unwrap();
        assert!(s.unchanged);
    }

    #[test]
    fn enable_disable_and_reconfigure() {
        let new = r#"
[core]
timezone = "Europe/Zurich"

[services.jellyfin]
enabled = true
subdomain = "media"

[services.immich]
enabled = false

[services.paperless]
enabled = true
"#;
        let s = summarize(Some(BASE), new).unwrap();
        assert_eq!(s.added, vec!["paperless"]);
        assert_eq!(s.removed, vec!["immich"]);
        assert_eq!(s.changed, vec!["jellyfin"]);
        assert_eq!(s.sections, vec!["core"]);
        assert!(!s.unchanged && !s.initial);
        assert_eq!(s.enabled, 2);
    }

    #[test]
    fn formatting_only_is_unchanged() {
        let new = "# comment\n[services.immich]\nenabled = true\n\n[services.jellyfin]\nsubdomain = \"tv\"\nenabled = true\n[core]\ntimezone = \"UTC\"\n[services.paperless]\nenabled = false\n";
        let s = summarize(Some(BASE), new).unwrap();
        assert!(s.unchanged, "{s:?}");
    }

    #[test]
    fn new_service_table_disabled_is_not_added() {
        let new = format!("{BASE}\n[services.vaultwarden]\nenabled = false\n");
        let s = summarize(Some(BASE), &new).unwrap();
        assert!(s.added.is_empty());
        assert!(s.unchanged);
    }

    #[test]
    fn unparsable_settings_give_no_summary() {
        assert!(summarize(Some(BASE), "not = [toml").is_none());
    }
}
