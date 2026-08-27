#[derive(Debug, Clone)]
pub(super) struct VisibleLine {
    pub(super) source_index: usize,
    pub(super) text: String,
}

pub(super) fn visible_lines(markdown: &str) -> Vec<VisibleLine> {
    let mut result = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut in_comment = false;
    let mut in_frontmatter = false;

    for (source_index, raw) in markdown.lines().enumerate() {
        let trimmed = raw.trim();
        if source_index == 0 && trimmed == "---" {
            in_frontmatter = true;
            continue;
        }
        if in_frontmatter {
            if trimmed == "---" || trimmed == "..." {
                in_frontmatter = false;
            }
            continue;
        }

        if let Some((marker, width)) = fence {
            if fence_width(trimmed, marker) >= width {
                fence = None;
            }
            continue;
        }
        if let Some((marker, width)) = fence_start(trimmed) {
            fence = Some((marker, width));
            continue;
        }
        if raw.starts_with("    ") || raw.starts_with('\t') {
            continue;
        }

        let visible = strip_html_comments(raw, &mut in_comment);
        let visible = visible.trim();
        if !visible.is_empty() {
            result.push(VisibleLine {
                source_index,
                text: visible.to_string(),
            });
        }
    }
    result
}

fn fence_start(line: &str) -> Option<(char, usize)> {
    for marker in ['`', '~'] {
        let width = fence_width(line, marker);
        if width >= 3 {
            return Some((marker, width));
        }
    }
    None
}

fn fence_width(line: &str, marker: char) -> usize {
    line.chars()
        .take_while(|character| *character == marker)
        .count()
}

fn strip_html_comments(line: &str, in_comment: &mut bool) -> String {
    let mut remaining = line;
    let mut output = String::new();
    loop {
        if *in_comment {
            let Some(end) = remaining.find("-->") else {
                return output;
            };
            remaining = &remaining[end + 3..];
            *in_comment = false;
            continue;
        }
        let Some(start) = remaining.find("<!--") else {
            output.push_str(remaining);
            return output;
        };
        output.push_str(&remaining[..start]);
        remaining = &remaining[start + 4..];
        *in_comment = true;
    }
}

#[cfg(test)]
mod tests {
    use super::visible_lines;

    #[test]
    fn ignores_non_rendered_markdown_blocks() {
        let markdown = "---\ntitle: hidden\n---\n~~~markdown\n## hidden\n~~~\n    [事实] hidden\n<!-- [建议] hidden -->\n## visible";
        let lines: Vec<String> = visible_lines(markdown)
            .into_iter()
            .map(|line| line.text)
            .collect();
        assert_eq!(lines, vec!["## visible"]);
    }
}
