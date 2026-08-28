use sha2::{Digest, Sha256};

pub(super) const MAX_PROJECT_IDENTITY_BYTES: usize = 256;

pub(super) struct PathValue {
    pub(super) key: String,
    pub(super) qualified: String,
    pub(super) encoded_fingerprint: String,
}

struct BoundedValue {
    prefix: String,
    original_bytes: usize,
    display_bytes: usize,
    hasher: Sha256,
}

impl BoundedValue {
    fn new() -> Self {
        Self {
            prefix: String::with_capacity(MAX_PROJECT_IDENTITY_BYTES),
            original_bytes: 0,
            display_bytes: 0,
            hasher: Sha256::new(),
        }
    }

    fn push_str(&mut self, value: &str) {
        self.original_bytes += value.len();
        self.hasher.update(value.as_bytes());
        for character in value.chars() {
            let mut bytes = [0; 4];
            let encoded = if character == '%' {
                "%%"
            } else {
                character.encode_utf8(&mut bytes)
            };
            self.display_bytes += encoded.len();
            if self.prefix.len() + encoded.len() <= MAX_PROJECT_IDENTITY_BYTES {
                self.prefix.push_str(encoded);
            }
        }
    }

    fn push_char(&mut self, value: char) {
        let mut bytes = [0; 4];
        self.push_str(value.encode_utf8(&mut bytes));
    }

    fn finish(self) -> String {
        self.finish_with_prefix("")
    }

    fn finish_with_prefix(mut self, identity_prefix: &str) -> String {
        if identity_prefix.len() + self.display_bytes <= MAX_PROJECT_IDENTITY_BYTES {
            return format!("{identity_prefix}{}", self.prefix);
        }
        let suffix = format!(
            "~%bytes={};sha256={:x}",
            self.original_bytes,
            self.hasher.finalize()
        );
        let max_value_prefix = MAX_PROJECT_IDENTITY_BYTES - identity_prefix.len() - suffix.len();
        while self.prefix.len() > max_value_prefix {
            self.prefix.pop();
        }
        if self
            .prefix
            .chars()
            .rev()
            .take_while(|char| *char == '%')
            .count()
            % 2
            == 1
        {
            self.prefix.pop();
        }
        format!("{identity_prefix}{}{suffix}", self.prefix)
    }

    fn fingerprint(self) -> String {
        format!(
            "{}:sha256:{:x}",
            self.original_bytes,
            self.hasher.finalize()
        )
    }
}

pub(super) fn bounded_hyphen_join<'a>(
    segments: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut value = BoundedValue::new();
    let mut populated = false;
    for segment in segments {
        if populated {
            value.push_char('-');
        }
        value.push_str(segment);
        populated = true;
    }
    populated.then(|| value.finish())
}

pub(super) fn raw_path_value(raw: &str, session_id: impl Fn(&str) -> bool) -> Option<PathValue> {
    let normalized = normalized_path_slice(raw, session_id)?;

    let mut path = BoundedValue::new();
    let mut raw_key = BoundedValue::new();
    for character in normalized.chars() {
        let normalized_character = if character == '\\' { '/' } else { character };
        path.push_char(normalized_character);
        raw_key.push_char(normalized_character);
    }

    let mut encoded = BoundedValue::new();
    let first = normalized.chars().next().map(encoded_character)?;
    if first != '-' {
        encoded.push_char('-');
    }
    for character in normalized.chars() {
        encoded.push_char(encoded_character(character));
    }

    Some(PathValue {
        key: format!("raw:{}", raw_key.fingerprint()),
        qualified: path.finish_with_prefix("path:"),
        encoded_fingerprint: encoded.fingerprint(),
    })
}

pub(super) fn raw_path_key(raw: &str, session_id: impl Fn(&str) -> bool) -> Option<String> {
    let normalized = normalized_path_slice(raw, session_id)?;
    let mut raw_key = BoundedValue::new();
    for character in normalized.chars() {
        raw_key.push_char(if character == '\\' { '/' } else { character });
    }
    Some(format!("raw:{}", raw_key.fingerprint()))
}

pub(super) fn encoded_path_value(
    raw: &str,
    session_id: impl Fn(&str) -> bool,
) -> Option<PathValue> {
    let normalized = normalized_encoded_slice(raw, session_id)?;
    let mut encoded = BoundedValue::new();
    encoded.push_str(normalized);
    let fingerprint = encoded.fingerprint();
    let mut qualified = BoundedValue::new();
    qualified.push_str(normalized);
    Some(PathValue {
        key: format!("encoded:{fingerprint}"),
        qualified: qualified.finish_with_prefix("encoded:"),
        encoded_fingerprint: fingerprint,
    })
}

pub(super) fn encoded_path_key(raw: &str, session_id: impl Fn(&str) -> bool) -> Option<String> {
    let normalized = normalized_encoded_slice(raw, session_id)?;
    let mut encoded = BoundedValue::new();
    encoded.push_str(normalized);
    Some(format!("encoded:{}", encoded.fingerprint()))
}

#[cfg(test)]
pub(super) fn normalized_path_display(
    raw: &str,
    session_id: impl Fn(&str) -> bool,
) -> Option<String> {
    raw_path_value(raw, session_id).map(|value| value.qualified["path:".len()..].to_string())
}

fn normalized_path_slice(raw: &str, session_id: impl Fn(&str) -> bool) -> Option<&str> {
    let mut normalized = raw.trim().trim_end_matches(['/', '\\']);
    if let Some(index) = normalized.rfind("-agent_") {
        if session_id(&normalized[index + 1..]) {
            normalized = &normalized[..index];
        }
    }
    (!normalized.is_empty()).then_some(normalized)
}

fn normalized_encoded_slice(raw: &str, session_id: impl Fn(&str) -> bool) -> Option<&str> {
    let mut encoded = raw.trim();
    if let Some(index) = encoded.rfind("-agent_") {
        if session_id(&encoded[index + 1..]) {
            encoded = &encoded[..index];
        }
    }
    (!encoded.is_empty()).then_some(encoded)
}

fn encoded_character(character: char) -> char {
    match character {
        '/' | '\\' | '.' | ':' => '-',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_path_identity_is_bounded_stable_and_collision_resistant() {
        let first = format!("/r/{}a", "x".repeat(1_021));
        let second = format!("/r/{}b", "x".repeat(1_021));
        assert_eq!(first.len(), 1_025);
        let first_value = raw_path_value(&first, |_| false).unwrap();
        let repeated = raw_path_value(&first, |_| false).unwrap();
        let second_value = raw_path_value(&second, |_| false).unwrap();

        assert_eq!(first_value.qualified, repeated.qualified);
        assert_ne!(first_value.qualified, second_value.qualified);
        assert!(first_value.qualified.len() <= MAX_PROJECT_IDENTITY_BYTES);
        assert!(first_value.qualified.starts_with("path:/r/"));
        assert!(first_value.qualified.contains("~%bytes=1025;sha256="));

        let literal = first_value.qualified.strip_prefix("path:").unwrap();
        let literal_value = raw_path_value(literal, |_| false).unwrap();
        assert_ne!(first_value.qualified, literal_value.qualified);
    }
}
