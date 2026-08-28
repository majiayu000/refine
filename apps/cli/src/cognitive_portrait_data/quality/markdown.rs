use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

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
    contains_html: bool,
}

pub(super) fn visible_blocks(markdown: &str) -> Vec<VisibleBlock> {
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
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
            Event::Start(Tag::Heading { level, .. }) if code_depth == 0 && quote_depth == 0 => {
                current = Some(VisibleBlock {
                    kind: VisibleBlockKind::Heading(heading_level(level)),
                    text: String::new(),
                    raw: markdown[range].to_string(),
                    contains_html: false,
                });
            }
            Event::Start(Tag::Paragraph) if code_depth == 0 && quote_depth == 0 => {
                current = Some(VisibleBlock {
                    kind: VisibleBlockKind::Paragraph,
                    text: String::new(),
                    raw: markdown[range].to_string(),
                    contains_html: false,
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
                if let Some(block) = current.as_mut() {
                    block.contains_html = true;
                }
            }
            Event::Code(_) => {}
            _ => {}
        }
    }
    finish_block(&mut current, &mut blocks);
    blocks
}

fn finish_block(current: &mut Option<VisibleBlock>, blocks: &mut Vec<VisibleBlock>) {
    if let Some(mut block) = current.take() {
        block.text = block.text.split_whitespace().collect::<Vec<_>>().join(" ");
        block.raw = block.raw.trim_end_matches(['\r', '\n']).to_string();
        if !block.text.is_empty() && !block.contains_html {
            blocks.push(block);
        }
    }
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
    use super::{visible_blocks, VisibleBlockKind};

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
}
