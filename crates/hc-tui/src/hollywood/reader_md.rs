//! Markdown-lite parser and path/SHA token splitter for the Lot Reader
//! (`spec.md` §2). Pure over lines — rendering lives in `reader_wrap.rs`.

use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MdBlock {
    Heading(String),
    Bullet(Vec<MdInline>),
    Paragraph(Vec<MdInline>),
    Code(String),
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MdInline {
    Text(String),
    Bold(String),
    Code(String),
    Teal(String),
}

/// Normalize a raw line: strip `\r` and control chars other than `\t`,
/// expanding `\t` to 2 spaces.
pub(super) fn normalize_line(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\r' => {}
            '\t' => out.push_str("  "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Parse lines into Markdown-lite blocks, collapsing >=3 consecutive blanks to 1.
pub(super) fn parse_markdown(lines: &[String]) -> Vec<MdBlock> {
    let mut raw_blocks = Vec::new();
    let mut in_fence = false;
    for raw in lines {
        let line = normalize_line(raw);
        let trimmed_end = line.trim_end();
        if trimmed_end.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            raw_blocks.push(MdBlock::Code(trimmed_end.to_owned()));
            continue;
        }
        if trimmed_end.trim().is_empty() {
            raw_blocks.push(MdBlock::Blank);
            continue;
        }
        let trimmed = trimmed_end.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed.get(hashes..).is_some_and(|r| r.starts_with(' ')) {
            raw_blocks.push(MdBlock::Heading(trimmed[hashes + 1..].trim().to_owned()));
        } else if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| strip_ordered_list(trimmed))
        {
            raw_blocks.push(MdBlock::Bullet(parse_inline(item)));
        } else {
            raw_blocks.push(MdBlock::Paragraph(parse_inline(trimmed)));
        }
    }
    collapse_blanks(raw_blocks)
}

fn strip_ordered_list(s: &str) -> Option<&str> {
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && s.get(digits..).is_some_and(|r| r.starts_with(". ")) {
        Some(&s[digits + 2..])
    } else {
        None
    }
}

fn collapse_blanks(blocks: Vec<MdBlock>) -> Vec<MdBlock> {
    let mut out = Vec::with_capacity(blocks.len());
    let mut blanks = 0usize;
    for block in blocks {
        if matches!(block, MdBlock::Blank) {
            blanks += 1;
        } else {
            push_blanks(&mut out, blanks);
            blanks = 0;
            out.push(block);
        }
    }
    push_blanks(&mut out, blanks);
    out
}

fn push_blanks(out: &mut Vec<MdBlock>, count: usize) {
    let emit = if count >= 3 { 1 } else { count };
    for _ in 0..emit {
        out.push(MdBlock::Blank);
    }
}

pub(super) fn inline_width(spans: &[MdInline]) -> usize {
    spans
        .iter()
        .map(|s| match s {
            MdInline::Text(t) | MdInline::Bold(t) | MdInline::Code(t) | MdInline::Teal(t) => {
                UnicodeWidthStr::width(t.as_str())
            }
        })
        .sum()
}

fn parse_inline(text: &str) -> Vec<MdInline> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['`', '*', '[']) {
        plain.push_str(&rest[..at]);
        rest = &rest[at..];
        let closed = match rest.as_bytes()[0] {
            b'`' => rest[1..]
                .find('`')
                .map(|end| (MdInline::Code(rest[1..1 + end].to_owned()), end + 2)),
            b'*' if rest.starts_with("**") => rest[2..]
                .find("**")
                .map(|end| (MdInline::Bold(rest[2..2 + end].to_owned()), end + 4)),
            b'[' => parse_link(rest),
            _ => None,
        };
        match closed {
            Some((span, used)) => {
                flush_plain(&mut plain, &mut spans);
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
    flush_plain(&mut plain, &mut spans);
    spans
}

fn parse_link(rest: &str) -> Option<(MdInline, usize)> {
    let close = rest.find("](")?;
    let end = rest[close..].find(')')? + close;
    let label = &rest[1..close];
    (!label.contains('[')).then(|| (MdInline::Teal(label.to_owned()), end + 1))
}

fn flush_plain(plain: &mut String, spans: &mut Vec<MdInline>) {
    if plain.is_empty() {
        return;
    }
    let taken = std::mem::take(plain);
    split_paths_and_shas(&taken, spans);
}

fn is_path_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | ':' | '~' | '-')
}

fn is_hex_sha(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_bare_path(s: &str) -> bool {
    if !s.contains('/') || s.contains("://") {
        return false;
    }
    let base = if let Some((head, tail)) = s.rsplit_once(':')
        && !tail.is_empty()
        && tail.bytes().all(|b| b.is_ascii_digit())
    {
        if let Some((head2, line_no)) = head.rsplit_once(':')
            && !line_no.is_empty()
            && line_no.bytes().all(|b| b.is_ascii_digit())
        {
            head2
        } else {
            head
        }
    } else {
        s
    };
    if base.starts_with('/') && base.matches('/').count() < 2 {
        return false;
    }
    let segs: Vec<&str> = base.split('/').collect();
    if base.ends_with('/') {
        return segs.iter().filter(|p| !p.is_empty()).count() >= 2;
    }
    let Some(last) = segs.last().copied() else {
        return false;
    };
    if segs[..segs.len() - 1].iter().all(|p| p.is_empty()) {
        return false;
    }
    let Some((stem, ext)) = last.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && (1..=10).contains(&ext.len())
        && ext.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn split_paths_and_shas(s: &str, out: &mut Vec<MdInline>) {
    let mut plain = String::new();
    let mut i = 0usize;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        let ch = s[i..].chars().next().unwrap();
        if is_path_token_char(ch) {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let nc = s[i..].chars().next().unwrap();
                if is_path_token_char(nc) {
                    i += nc.len_utf8();
                } else {
                    break;
                }
            }
            let raw_tok = &s[start..i];
            let trimmed = raw_tok.trim_end_matches(['.', ',', ':', ';', '!', '?']);
            let lead_trim = trimmed.trim_start_matches(['.', ':', '-']);
            let prefix_len = trimmed.len() - lead_trim.len();
            let core = &trimmed[prefix_len..];
            if !core.is_empty() && (is_hex_sha(core) || is_bare_path(core)) {
                plain.push_str(&raw_tok[..prefix_len]);
                if !plain.is_empty() {
                    out.push(MdInline::Text(std::mem::take(&mut plain)));
                }
                out.push(MdInline::Teal(core.to_owned()));
                plain.push_str(&raw_tok[trimmed.len()..]);
            } else {
                plain.push_str(raw_tok);
            }
        } else {
            plain.push(ch);
            i += ch.len_utf8();
        }
    }
    if !plain.is_empty() {
        out.push(MdInline::Text(plain));
    }
}
