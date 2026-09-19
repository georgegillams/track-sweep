use std::collections::HashMap;

/// Lowercase, drop `(…)`, `[…]`, and `{…}`, strip punctuation, collapse whitespace.
pub fn normalize_name(name: &str) -> String {
    let without_brackets = strip_bracketed_sections(name);
    without_brackets
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Ids of tracks that share a normalized name with at least one other track.
/// Groups are in first-seen order; each group is original then later matches.
pub fn duplicate_ids<'a, I>(tracks: I) -> Vec<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    let mut order = Vec::new();
    for (id, name) in tracks {
        let key = normalize_name(name);
        if key.is_empty() {
            continue;
        }
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(id.to_string());
    }

    let mut duplicates = Vec::new();
    for key in order {
        let group = &groups[&key];
        if group.len() > 1 {
            duplicates.extend(group.iter().cloned());
        }
    }
    duplicates
}

fn strip_bracketed_sections(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut closers = Vec::new();
    for c in name.chars() {
        match c {
            '(' => closers.push(')'),
            '[' => closers.push(']'),
            '{' => closers.push('}'),
            ')' | ']' | '}' => {
                if closers.last() == Some(&c) {
                    closers.pop();
                } else if closers.is_empty() {
                    out.push(c);
                }
            }
            _ if closers.is_empty() => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_parens_punctuation_and_case() {
        assert_eq!(normalize_name("Hello (Remix)!"), "hello");
        assert_eq!(normalize_name("Mr. Brightside"), "mr brightside");
        assert_eq!(normalize_name("Don't Stop"), "dont stop");
        assert_eq!(normalize_name("  Song   Title  "), "song title");
        assert_eq!(normalize_name("Hello (Live) (Remastered)"), "hello");
        assert_eq!(normalize_name("Hello [Live]"), "hello");
        assert_eq!(normalize_name("Hello {Deluxe Edition}"), "hello");
        assert_eq!(normalize_name("Song (feat. A) [Remastered] {Mono}"), "song");
        assert_eq!(normalize_name("Café"), "café");
    }

    #[test]
    fn nested_and_empty_brackets() {
        assert_eq!(normalize_name("Track (Live (Edit))"), "track");
        assert_eq!(normalize_name("Track [Live {Edit}]"), "track");
        assert_eq!(normalize_name("(Intro)"), "");
        assert_eq!(normalize_name("[Silence]"), "");
        assert_eq!(normalize_name("{Bonus}"), "");
        assert_eq!(normalize_name("???"), "");
    }

    #[test]
    fn includes_original_and_later_matches() {
        let tracks = [
            ("1", "Hello"),
            ("2", "Goodbye"),
            ("3", "Hello (Remix)"),
            ("4", "hello!"),
            ("5", "Goodbye"),
            ("6", "Unique"),
        ];
        let ids = duplicate_ids(tracks.iter().copied());
        assert_eq!(ids, vec!["1", "3", "4", "2", "5"]);
    }

    #[test]
    fn empty_normalized_names_are_ignored() {
        let tracks = [("1", "(foo)"), ("2", "[bar]"), ("3", "{baz}"), ("4", "???")];
        assert!(duplicate_ids(tracks.iter().copied()).is_empty());
    }
}
