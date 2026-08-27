use std::collections::HashSet;

use super::markdown::visible_lines;

pub(super) fn novelty_rate(candidate: &str, previous: &str) -> f64 {
    let candidate = normalized_paragraphs(candidate);
    if candidate.is_empty() {
        return 0.0;
    }
    let previous: HashSet<String> = normalized_paragraphs(previous).into_iter().collect();
    let novel = candidate
        .iter()
        .filter(|paragraph| !previous.contains(*paragraph))
        .count();
    novel as f64 / candidate.len() as f64
}

fn normalized_paragraphs(report: &str) -> Vec<String> {
    visible_lines(report)
        .into_iter()
        .map(|line| line.text)
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('#')
                || line.starts_with("---")
                || line.starts_with('|')
                || is_reference_definition(line)
            {
                return None;
            }
            let body = line.to_string();
            let stripped = strip_iso_dates(&strip_render_metadata(&body));
            if stripped.trim().is_empty() {
                return None;
            }
            let normalized: String = stripped
                .chars()
                .filter(|character| character.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect();
            (normalized.chars().count() >= 20).then_some(normalized)
        })
        .collect()
}

fn is_reference_definition(line: &str) -> bool {
    line.starts_with('[')
        && line
            .find("]:")
            .is_some_and(|end| end > 1 && !line[..end].contains(' '))
}

fn strip_render_metadata(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < characters.len() {
        if characters[index] == '<' {
            if let Some(end) = characters[index + 1..]
                .iter()
                .position(|character| *character == '>')
            {
                index += end + 2;
                continue;
            }
        }
        if characters[index] == '&' {
            if let Some(end) = characters[index + 1..]
                .iter()
                .take(32)
                .position(|character| *character == ';')
            {
                index += end + 2;
                continue;
            }
        }
        if characters[index] == '[' {
            if let Some(end) = characters[index + 1..]
                .iter()
                .position(|character| *character == ']')
            {
                let metadata: String = characters[index + 1..index + end + 1].iter().collect();
                if is_machine_metadata(&metadata) {
                    index += end + 2;
                    continue;
                }
            }
        }
        if characters[index] == ']' && characters.get(index + 1) == Some(&'(') {
            if let Some(end) = characters[index + 2..]
                .iter()
                .position(|character| *character == ')')
            {
                output.push(']');
                index += end + 3;
                continue;
            }
        }
        if characters[index] == ']' && characters.get(index + 1) == Some(&'[') {
            if let Some(end) = characters[index + 2..]
                .iter()
                .position(|character| *character == ']')
            {
                output.push(']');
                index += end + 3;
                continue;
            }
        }
        output.push(characters[index]);
        index += 1;
    }
    output
}

fn strip_iso_dates(value: &str) -> String {
    let characters: Vec<char> = value.chars().collect();
    let mut output = String::with_capacity(value.len());
    let mut index = 0usize;
    while index < characters.len() {
        let is_date = index + 10 <= characters.len()
            && characters[index..index + 4]
                .iter()
                .all(char::is_ascii_digit)
            && characters[index + 4] == '-'
            && characters[index + 5..index + 7]
                .iter()
                .all(char::is_ascii_digit)
            && characters[index + 7] == '-'
            && characters[index + 8..index + 10]
                .iter()
                .all(char::is_ascii_digit);
        if is_date {
            index += 10;
            if characters.get(index) == Some(&'T') {
                index += 1;
                while characters.get(index).is_some_and(|character| {
                    character.is_ascii_digit() || matches!(character, ':' | '.' | '+' | '-' | 'Z')
                }) {
                    index += 1;
                }
            }
            continue;
        }
        output.push(characters[index]);
        index += 1;
    }
    output
}

fn is_machine_metadata(value: &str) -> bool {
    value == "事实"
        || value == "建议"
        || value == "趋势"
        || value == "趋势抑制"
        || value.starts_with("推断")
        || value.starts_with("evidence:")
        || value.starts_with("bundle:")
        || value.starts_with("metric:")
        || value.starts_with("owner:")
        || value.starts_with("due:")
        || value.starts_with("verify:")
}
