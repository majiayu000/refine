use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

pub(super) const MAX_MARKDOWN_BLOCKS: usize = 4096;
pub(super) const MAX_MARKDOWN_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum VisibleBlockKind {
    Heading(u8),
    Paragraph,
}

#[derive(Debug, Clone)]
pub(super) struct VisibleBlock {
    pub(super) kind: VisibleBlockKind,
    pub(super) text: String,
    pub(super) raw: String,
}

pub(super) struct MarkdownScan {
    pub(super) blocks: Vec<VisibleBlock>,
    pub(super) violations: Vec<String>,
}

pub(super) fn scan_markdown(markdown: &str) -> MarkdownScan {
    let mut violations = Vec::new();
    if markdown
        .split(['\n', '\r'])
        .any(|line| line.len() > MAX_MARKDOWN_LINE_BYTES)
    {
        violations.push(format!(
            "Markdown line exceeds the {MAX_MARKDOWN_LINE_BYTES} byte limit"
        ));
    }
    let markdown = strip_frontmatter(markdown);
    let parser = Parser::new(markdown);
    let mut blocks = Vec::new();
    let mut current: Option<VisibleBlock> = None;
    let mut code_depth = 0usize;
    let mut quote_depth = 0usize;
    let mut image_depth = 0usize;

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => code_depth += 1,
            Event::End(TagEnd::CodeBlock) => code_depth = code_depth.saturating_sub(1),
            Event::Start(Tag::BlockQuote(_)) => quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => quote_depth = quote_depth.saturating_sub(1),
            Event::Start(Tag::Image { .. }) => {
                violations.push("Markdown images are forbidden in portrait archives".to_string());
                image_depth += 1;
            }
            Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) if !safe_link_destination(&dest_url) => {
                violations.push(format!(
                    "unsafe Markdown link destination is forbidden: {dest_url}"
                ));
            }
            Event::Start(Tag::Heading { level, .. }) if code_depth == 0 && quote_depth == 0 => {
                current = Some(VisibleBlock {
                    kind: VisibleBlockKind::Heading(heading_level(level)),
                    text: String::new(),
                    raw: markdown[range].to_string(),
                });
            }
            Event::Start(Tag::Paragraph) if code_depth == 0 && quote_depth == 0 => {
                current = Some(VisibleBlock {
                    kind: VisibleBlockKind::Paragraph,
                    text: String::new(),
                    raw: markdown[range].to_string(),
                });
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::Paragraph)
                if code_depth == 0 && quote_depth == 0 =>
            {
                finish_block(&mut current, &mut blocks);
            }
            Event::Text(text) if code_depth == 0 && quote_depth == 0 && image_depth == 0 => {
                if let Some(block) = current.as_mut() {
                    block.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak
                if code_depth == 0 && quote_depth == 0 && image_depth == 0 =>
            {
                if let Some(block) = current.as_mut() {
                    block.text.push(' ');
                }
            }
            Event::Html(_) | Event::InlineHtml(_) => {
                violations.push("raw HTML is forbidden in portrait archives".to_string());
            }
            Event::Code(_) => {}
            _ => {}
        }
        if blocks.len() > MAX_MARKDOWN_BLOCKS {
            blocks.truncate(MAX_MARKDOWN_BLOCKS);
            violations.push(format!(
                "Markdown exceeds the {MAX_MARKDOWN_BLOCKS} rendered block limit"
            ));
            break;
        }
    }
    finish_block(&mut current, &mut blocks);
    if blocks.len() > MAX_MARKDOWN_BLOCKS {
        blocks.truncate(MAX_MARKDOWN_BLOCKS);
        violations.push(format!(
            "Markdown exceeds the {MAX_MARKDOWN_BLOCKS} rendered block limit"
        ));
    }
    violations.sort();
    violations.dedup();
    MarkdownScan { blocks, violations }
}

pub(super) fn visible_blocks(markdown: &str) -> Vec<VisibleBlock> {
    scan_markdown(markdown).blocks
}

fn finish_block(current: &mut Option<VisibleBlock>, blocks: &mut Vec<VisibleBlock>) {
    if let Some(mut block) = current.take() {
        block.text = block.text.split_whitespace().collect::<Vec<_>>().join(" ");
        block.raw = block.raw.trim_end_matches(['\r', '\n']).to_string();
        if !block.text.is_empty() {
            blocks.push(block);
        }
    }
}

fn safe_link_destination(destination: &str) -> bool {
    let destination = destination.trim();
    if destination.chars().any(char::is_control)
        || destination.starts_with('/')
        || destination.contains('\\')
    {
        return false;
    }
    let lowercase = destination.to_ascii_lowercase();
    if lowercase.starts_with("http://") || lowercase.starts_with("https://") {
        return true;
    }
    if destination.contains(':') {
        return false;
    }
    let relative_path = destination.split(['?', '#']).next().unwrap_or_default();
    !relative_path.split('/').any(|component| component == "..")
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn strip_frontmatter(markdown: &str) -> &str {
    for (opening, closing) in [("---\n", "\n---\n"), ("---\r\n", "\r\n---\r\n")] {
        if let Some(rest) = markdown.strip_prefix(opening) {
            return rest
                .find(closing)
                .map_or("", |end| &rest[end + closing.len()..]);
        }
    }
    markdown
}

#[cfg(test)]
mod tests {
    use super::{scan_markdown, visible_blocks, VisibleBlockKind};

    #[test]
    fn commonmark_parser_ignores_non_rendered_and_quoted_claims() {
        let markdown = "---\ntitle: hidden\n---\n~~~markdown\n## hidden\n~~~not-a-close\n## still hidden\n~~~\n    [事实] hidden\n> [建议] quoted\n<!-- [建议] hidden -->\n## visible\nsoft\nwrapped";
        let blocks = visible_blocks(markdown);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, VisibleBlockKind::Heading(2));
        assert_eq!(blocks[0].text, "visible");
        assert_eq!(blocks[1].kind, VisibleBlockKind::Paragraph);
        assert_eq!(blocks[1].text, "soft wrapped");
        assert_eq!(blocks[1].raw, "soft\nwrapped");
    }

    #[test]
    fn active_content_and_unsafe_links_are_reported() {
        let scan = scan_markdown(
            "text <span>visible</span>\n\n![track](https://example.com/x)\n\n[x](javascript:alert(1))\n\n[y](../outside)",
        );
        assert_eq!(scan.blocks[0].text, "text visible");
        assert!(scan
            .violations
            .iter()
            .any(|error| error.contains("raw HTML")));
        assert!(scan.violations.iter().any(|error| error.contains("images")));
        assert!(scan
            .violations
            .iter()
            .any(|error| error.contains("unsafe Markdown link")));
    }
}
