//! Parser for the release-notes Markdown subset: `#` headings, `- `
//! and `* ` bullets, `**bold**` lead-ins, `` `code` `` spans, fenced
//! code blocks, and `[text](url)` links rendered as their text. Pure
//! and allocation-light; the renderer lives in `chrome/release_notes`.

/// One logical line of the notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Block {
    /// `# …` through `###### …`, text without the hashes.
    Heading(String),
    /// A `- ` or `* ` item.
    Bullet(Vec<Inline>),
    /// Any other non-empty line.
    Paragraph(Vec<Inline>),
    /// One line inside a fenced block (verbatim, blank lines kept).
    Code(String),
    /// An empty line outside a fence.
    Blank,
}

/// The inline spans a line is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Inline {
    Text(String),
    Bold(String),
    Code(String),
}

/// Splits `body` into blocks. Unclosed constructs degrade to plain
/// text: an open fence runs to the end, an unclosed `**` or `` ` `` is
/// literal.
pub(crate) fn parse(body: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut in_fence = false;
    for raw in body.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            blocks.push(Block::Code(line.to_owned()));
            continue;
        }
        if line.trim().is_empty() {
            blocks.push(Block::Blank);
            continue;
        }
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            blocks.push(Block::Heading(trimmed[hashes..].trim().to_owned()));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            blocks.push(Block::Bullet(inline(item)));
        } else {
            blocks.push(Block::Paragraph(inline(trimmed)));
        }
    }
    blocks
}

/// The inline spans of one line, in order.
pub(crate) fn inline(text: &str) -> Vec<Inline> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['`', '*', '[']) {
        plain.push_str(&rest[..at]);
        rest = &rest[at..];
        let closed = match rest.as_bytes()[0] {
            b'`' => rest[1..]
                .find('`')
                .map(|end| (Inline::Code(rest[1..1 + end].to_owned()), end + 2)),
            b'*' if rest.starts_with("**") => rest[2..]
                .find("**")
                .map(|end| (Inline::Bold(rest[2..2 + end].to_owned()), end + 4)),
            b'[' => link(rest),
            _ => None,
        };
        match closed {
            Some((span, used)) => {
                flush(&mut plain, &mut spans);
                spans.push(span);
                rest = &rest[used..];
            }
            None => {
                let used = rest.chars().next().map_or(1, char::len_utf8);
                plain.push_str(&rest[..used]);
                rest = &rest[used..];
            }
        }
    }
    plain.push_str(rest);
    flush(&mut plain, &mut spans);
    spans
}

/// `[text](target)` at the start of `rest`: the link's text and the
/// number of bytes it spans.
fn link(rest: &str) -> Option<(Inline, usize)> {
    let close = rest.find("](")?;
    let end = rest[close..].find(')')? + close;
    let label = &rest[1..close];
    (!label.contains('[')).then(|| (Inline::Text(label.to_owned()), end + 1))
}

fn flush(plain: &mut String, spans: &mut Vec<Inline>) {
    if !plain.is_empty() {
        spans.push(Inline::Text(std::mem::take(plain)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn bold_lead_in_inline_code_fence_and_link_parse() {
        let body = "### Added\n- **Waiting status:** show `waiting` while [the agent](https://example.invalid/x) pauses (#39)\n\n```sh\nhc-tui --upgrade\n\n- not a bullet\n```\nPlain line";
        let blocks = parse(body);
        assert_eq!(blocks[0], Block::Heading("Added".into()));
        assert_eq!(
            blocks[1],
            Block::Bullet(vec![
                Inline::Bold("Waiting status:".into()),
                text(" show "),
                Inline::Code("waiting".into()),
                text(" while "),
                text("the agent"),
                text(" pauses (#39)"),
            ])
        );
        assert_eq!(blocks[2], Block::Blank);
        assert_eq!(blocks[3], Block::Code("hc-tui --upgrade".into()));
        assert_eq!(blocks[4], Block::Code(String::new()));
        assert_eq!(blocks[5], Block::Code("- not a bullet".into()));
        assert_eq!(blocks[6], Block::Paragraph(vec![text("Plain line")]));
    }

    #[test]
    fn unclosed_markers_are_literal_text() {
        assert_eq!(
            inline("a `open and **open [bracket"),
            vec![text("a `open and **open [bracket")]
        );
    }

    #[test]
    fn an_open_fence_runs_to_the_end() {
        let blocks = parse("```\nline");
        assert_eq!(blocks, vec![Block::Code("line".into())]);
    }

    #[test]
    fn asterisk_bullets_and_all_heading_levels() {
        let blocks = parse("## Fixed\n* one\n###### six\n#nope");
        assert_eq!(blocks[0], Block::Heading("Fixed".into()));
        assert_eq!(blocks[1], Block::Bullet(vec![text("one")]));
        assert_eq!(blocks[2], Block::Heading("six".into()));
        assert_eq!(blocks[3], Block::Paragraph(vec![text("#nope")]));
    }
}
