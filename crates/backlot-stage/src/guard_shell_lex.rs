//! Shell segment lexing with quote/escape validation and command-substitution
//! extraction for `guard_shell.rs`.

pub(crate) struct Heredoc {
    pub seg_idx: usize,
    pub pipe_id: usize,
    pub body: String,
}

struct PendingHeredoc {
    seg_idx: usize,
    pipe_id: usize,
    delim: String,
    quoted: bool,
    strip_tabs: bool,
}

pub(crate) struct ParsedLine {
    pub segments: Vec<Vec<String>>,
    pub pipe_ids: Vec<usize>,
    pub nested: Vec<String>,
    pub heredocs: Vec<Heredoc>,
}

pub(crate) fn split_shell_segments_checked(line: &str) -> Result<ParsedLine, &'static str> {
    let mut segs = Vec::new();
    let mut pipe_ids = Vec::new();
    let mut nested = Vec::new();
    let mut heredocs = Vec::new();
    let mut pending_heredocs = Vec::new();
    let mut seg = Vec::new();
    let mut pipe_id = 0usize;
    let mut word = String::new();
    let mut has_word = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    let mut chars = line.chars().peekable();

    let flush_word = |seg: &mut Vec<String>, word: &mut String, has_word: &mut bool| {
        if *has_word {
            seg.push(std::mem::take(word));
            *has_word = false;
        }
    };
    let flush_seg = |segs: &mut Vec<Vec<String>>,
                     pipe_ids: &mut Vec<usize>,
                     seg: &mut Vec<String>,
                     pid: usize| {
        if !seg.is_empty() {
            segs.push(std::mem::take(seg));
            pipe_ids.push(pid);
        }
    };

    while let Some(c) = chars.next() {
        if escape {
            escape = false;
            if c != '\n' {
                word.push(c);
                has_word = true;
            }
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            } else {
                word.push(c);
                has_word = true;
            }
            continue;
        }
        if c == '$' && chars.peek() == Some(&'(') {
            chars.next();
            let sub = read_cmd_subst(&mut chars)?;
            nested.push(sub);
            has_word = true;
            continue;
        }
        if c == '`' {
            let sub = read_backtick(&mut chars)?;
            nested.push(sub);
            has_word = true;
            continue;
        }
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => {
                    word.push(c);
                    has_word = true;
                }
            }
            continue;
        }
        if c == '<' && chars.peek() == Some(&'<') {
            chars.next();
            if chars.peek() == Some(&'<') {
                chars.next();
                word.push_str("<<<");
                has_word = true;
            } else {
                if has_word && !word.chars().all(|d| d.is_ascii_digit()) {
                    flush_word(&mut seg, &mut word, &mut has_word);
                }
                let (strip_tabs, delim, quoted) = read_heredoc_spec(&mut chars)?;
                word.push_str(if strip_tabs { "<<-" } else { "<<" });
                word.push_str(&delim);
                has_word = true;
                pending_heredocs.push(PendingHeredoc {
                    seg_idx: segs.len(),
                    pipe_id,
                    delim,
                    quoted,
                    strip_tabs,
                });
            }
            continue;
        }
        match c {
            '#' if !has_word => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            '\'' => {
                in_single = true;
                has_word = true;
            }
            '"' => {
                in_double = true;
                has_word = true;
            }
            '\\' => escape = true,
            '\n' => {
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut pipe_ids, &mut seg, pipe_id);
                pipe_id += 1;
                if !pending_heredocs.is_empty() {
                    consume_heredocs(
                        &mut chars,
                        &mut pending_heredocs,
                        &mut heredocs,
                        &mut nested,
                    )?;
                }
            }
            ';' => {
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut pipe_ids, &mut seg, pipe_id);
                pipe_id += 1;
            }
            '(' | ')' => {
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut pipe_ids, &mut seg, pipe_id);
            }
            '&' | '|' => {
                let doubled = chars.peek() == Some(&c);
                if doubled {
                    chars.next();
                }
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut pipe_ids, &mut seg, pipe_id);
                if c == '&' || doubled {
                    pipe_id += 1;
                }
            }
            _ if c.is_whitespace() => {
                flush_word(&mut seg, &mut word, &mut has_word);
            }
            _ => {
                word.push(c);
                has_word = true;
            }
        }
    }
    if escape || in_single || in_double || !pending_heredocs.is_empty() {
        return Err("unparseable shell syntax");
    }
    flush_word(&mut seg, &mut word, &mut has_word);
    flush_seg(&mut segs, &mut pipe_ids, &mut seg, pipe_id);
    Ok(ParsedLine {
        segments: segs,
        pipe_ids,
        nested,
        heredocs,
    })
}

fn read_heredoc_spec(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<(bool, String, bool), &'static str> {
    let strip_tabs = chars.peek() == Some(&'-');
    if strip_tabs {
        chars.next();
    }
    while chars.peek().is_some_and(|&n| n == ' ' || n == '\t') {
        chars.next();
    }
    let mut delim = String::new();
    let mut quoted = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    while let Some(&nc) = chars.peek() {
        if escape {
            chars.next();
            delim.push(nc);
            escape = false;
            continue;
        }
        if in_single {
            chars.next();
            if nc == '\'' {
                in_single = false;
            } else {
                delim.push(nc);
            }
            continue;
        }
        if in_double {
            chars.next();
            match nc {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => delim.push(nc),
            }
            continue;
        }
        if nc.is_whitespace() || matches!(nc, ';' | '&' | '|' | '(' | ')' | '<' | '>') {
            break;
        }
        chars.next();
        match nc {
            '\'' => {
                in_single = true;
                quoted = true;
            }
            '"' => {
                in_double = true;
                quoted = true;
            }
            '\\' => {
                escape = true;
                quoted = true;
            }
            _ => delim.push(nc),
        }
    }
    if escape || in_single || in_double || delim.is_empty() {
        return Err("unparseable shell syntax");
    }
    Ok((strip_tabs, delim, quoted))
}

fn is_heredoc_delim(line: &str, delim: &str, strip_tabs: bool) -> bool {
    let trimmed = if strip_tabs {
        line.trim_start_matches('\t')
    } else {
        line
    };
    trimmed == delim
}

fn consume_heredocs(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    pending: &mut Vec<PendingHeredoc>,
    heredocs: &mut Vec<Heredoc>,
    nested: &mut Vec<String>,
) -> Result<(), &'static str> {
    for p in pending.drain(..) {
        let mut body = String::new();
        let mut line = String::new();
        let mut closed = false;
        for c in chars.by_ref() {
            if c == '\n' {
                if is_heredoc_delim(&line, &p.delim, p.strip_tabs) {
                    closed = true;
                    line.clear();
                    break;
                }
                body.push_str(&line);
                body.push('\n');
                line.clear();
            } else {
                line.push(c);
            }
        }
        if !closed && is_heredoc_delim(&line, &p.delim, p.strip_tabs) {
            closed = true;
        }
        if !closed {
            return Err("unparseable shell syntax");
        }
        if !p.quoted {
            extract_unquoted_substitutions(&body, nested)?;
        }
        heredocs.push(Heredoc {
            seg_idx: p.seg_idx,
            pipe_id: p.pipe_id,
            body,
        });
    }
    Ok(())
}

fn extract_unquoted_substitutions(
    body: &str,
    nested: &mut Vec<String>,
) -> Result<(), &'static str> {
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c == '$' && chars.peek() == Some(&'(') {
            chars.next();
            nested.push(read_cmd_subst(&mut chars)?);
        } else if c == '`' {
            nested.push(read_backtick(&mut chars)?);
        }
    }
    Ok(())
}

fn read_cmd_subst(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, &'static str> {
    let mut depth = 1usize;
    let mut out = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;
    for c in chars.by_ref() {
        if escape {
            escape = false;
            out.push(c);
            continue;
        }
        if in_single {
            if c == '\'' {
                in_single = false;
            }
            out.push(c);
            continue;
        }
        if in_double {
            match c {
                '"' => in_double = false,
                '\\' => escape = true,
                _ => {}
            }
            out.push(c);
            continue;
        }
        match c {
            '\'' => in_single = true,
            '"' => in_double = true,
            '\\' => escape = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(out);
                }
            }
            _ => {}
        }
        out.push(c);
    }
    Err("unparseable shell syntax")
}

fn read_backtick(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) -> Result<String, &'static str> {
    let mut out = String::new();
    let mut escape = false;
    for c in chars.by_ref() {
        if escape {
            escape = false;
            out.push(c);
            continue;
        }
        match c {
            '\\' => {
                escape = true;
                out.push(c);
            }
            '`' => return Ok(out),
            _ => out.push(c),
        }
    }
    Err("unparseable shell syntax")
}
