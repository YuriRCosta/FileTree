use regex::Regex;
use std::sync::OnceLock;

fn control(character: char) -> bool {
    matches!(character, '\u{0}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}' | '\u{7f}')
}

pub fn clean(value: &str, limit: usize) -> String {
    let mut collapsed = String::with_capacity(value.len());
    let mut pending = false;
    for character in value.chars() {
        if control(character) {
            continue;
        }
        if character.is_whitespace() {
            pending = !collapsed.is_empty();
            continue;
        }
        if pending {
            collapsed.push(' ');
            pending = false;
        }
        collapsed.push(character);
    }
    collapsed.chars().take(limit).collect()
}

pub fn estimated_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

pub fn word_count(text: &str) -> usize {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"\w+").expect("word pattern"))
        .find_iter(text)
        .count()
}
