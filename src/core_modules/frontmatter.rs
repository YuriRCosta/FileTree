use super::text::clean;
use regex::Regex;
use std::sync::OnceLock;

pub const MAX_FRONTMATTER_CHARS: usize = 32 * 1024;
pub const MAX_DETAIL_CHARS: usize = 160;
const FIELD_LIMIT: usize = 1 << 16;
const BLOCK_MARKERS: [&str; 6] = [">", "|", ">-", "|-", ">+", "|+"];

fn delimiter(line: &str) -> bool {
    line.strip_prefix("---")
        .map(|rest| rest.strip_suffix('\r').unwrap_or(rest))
        .is_some_and(|rest| {
            rest.chars()
                .all(|character| character == ' ' || character == '\t')
        })
}

pub fn block(text: &str) -> String {
    let head: String = text.chars().take(MAX_FRONTMATTER_CHARS).collect();
    let Some((first, rest)) = head.split_once('\n') else {
        return String::new();
    };
    if !delimiter(first) {
        return String::new();
    }
    let mut collected: Vec<&str> = Vec::new();
    for line in rest.split('\n') {
        if delimiter(line) {
            return collected.join("\n");
        }
        collected.push(line);
    }
    String::new()
}

fn trailing_comment() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\s+#.*$").expect("trailing comment pattern"))
}

fn unquoted(value: &str) -> &str {
    value.trim_matches('"').trim_matches('\'')
}

fn scalar(first: &str) -> String {
    if first.starts_with('#') {
        return String::new();
    }
    let without_comment = trailing_comment().replace(first, "");
    clean(unquoted(without_comment.trim()), FIELD_LIMIT)
}

pub fn value(text: &str, key: &str) -> String {
    let source = block(text);
    if source.is_empty() {
        return String::new();
    }
    let mut lines = source.split('\n');
    let mut found = None;
    for line in lines.by_ref() {
        let Some(rest) = line
            .strip_prefix(key)
            .map(|rest| rest.trim_start_matches([' ', '\t']))
            .and_then(|rest| rest.strip_prefix(':'))
        else {
            continue;
        };
        found = Some(rest.trim().to_string());
        break;
    }
    let Some(first) = found else {
        return String::new();
    };
    if BLOCK_MARKERS.contains(&first.as_str()) {
        let mut folded: Vec<&str> = Vec::new();
        for line in lines {
            if !line.trim().is_empty()
                && !line
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_whitespace())
            {
                break;
            }
            folded.push(line.trim());
        }
        return clean(&folded.join(" "), FIELD_LIMIT);
    }
    scalar(&first)
}

pub fn frontmatter_field(text: &str, key: &str) -> String {
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    if !lines.next().is_some_and(|line| line.trim() == "---") {
        return String::new();
    }
    for line in lines {
        let stripped = line.trim();
        if stripped == "---" {
            return String::new();
        }
        let Some((name, value)) = stripped.split_once(':') else {
            continue;
        };
        if name.trim() == key {
            return unquoted(value.trim())
                .chars()
                .take(MAX_DETAIL_CHARS)
                .collect();
        }
    }
    String::new()
}
