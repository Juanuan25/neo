//! Word-level intra-line highlighting for paired removed/added lines.

/// A run of text; `changed` marks tokens not shared with the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub changed: bool,
}

/// Token budget per side; longer lines are highlighted as a whole.
const MAX_TOKENS: usize = 300;
/// Minimum share of unchanged characters for highlighting to be useful.
const MIN_COMMON_RATIO: f64 = 0.3;

/// Split into word runs, whitespace runs and single punctuation characters.
fn tokenize(s: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Punct,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() {
            Class::Space
        } else {
            Class::Punct
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<Class> = None;
    for (i, c) in s.char_indices() {
        let k = class(c);
        let split = match prev {
            None => false,
            Some(p) => p != k || k == Class::Punct,
        };
        if split {
            out.push(&s[start..i]);
            start = i;
        }
        prev = Some(k);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn push_segment(out: &mut Vec<Segment>, text: &str, changed: bool) {
    if let Some(last) = out.last_mut() {
        if last.changed == changed {
            last.text.push_str(text);
            return;
        }
    }
    out.push(Segment {
        text: text.to_string(),
        changed,
    });
}

/// Word diff of `old` vs `new`. `None` when the lines are too different (or too long)
/// for highlighting to help — the caller then renders plain add/del lines.
pub fn word_diff(old: &str, new: &str) -> Option<(Vec<Segment>, Vec<Segment>)> {
    if old == new {
        return None;
    }
    let a = tokenize(old);
    let b = tokenize(new);
    if a.len() > MAX_TOKENS || b.len() > MAX_TOKENS || a.is_empty() || b.is_empty() {
        return None;
    }
    let (n, m) = (a.len(), b.len());
    // LCS table (suffix lengths), (n+1)*(m+1) ≤ ~90k cells.
    let w = m + 1;
    let mut dp = vec![0u16; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i * w + j] = if a[i] == b[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut common = 0usize;
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            if !a[i].trim().is_empty() {
                common += a[i].len();
            }
            push_segment(&mut left, a[i], false);
            push_segment(&mut right, b[j], false);
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            push_segment(&mut left, a[i], true);
            i += 1;
        } else {
            push_segment(&mut right, b[j], true);
            j += 1;
        }
    }
    for t in &a[i..] {
        push_segment(&mut left, t, true);
    }
    for t in &b[j..] {
        push_segment(&mut right, t, true);
    }
    let longest = old.trim().len().max(new.trim().len()).max(1);
    if (common as f64) / (longest as f64) < MIN_COMMON_RATIO {
        return None;
    }
    Some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(segs: &[Segment]) -> Vec<&str> {
        segs.iter()
            .filter(|s| s.changed)
            .map(|s| s.text.as_str())
            .collect()
    }

    #[test]
    fn highlights_changed_value_only() {
        let (l, r) = word_diff(r#"hostname = "old""#, r#"hostname = "new""#).expect("diff");
        assert_eq!(changed(&l), vec!["old"]);
        assert_eq!(changed(&r), vec!["new"]);
        let joined: String = r.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, r#"hostname = "new""#);
    }

    #[test]
    fn unrelated_lines_are_not_highlighted() {
        assert!(word_diff("enabled = true", "[services.jellyfin]").is_none());
        assert!(word_diff("same", "same").is_none());
    }

    #[test]
    fn tokenizer_keeps_all_text() {
        let s = "a.b  = [\"x\", 12]";
        assert_eq!(tokenize(s).concat(), s);
    }
}
