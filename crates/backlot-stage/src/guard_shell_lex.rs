//! Shell segment lexing with quote/escape validation and command-substitution
//! extraction for `guard_shell.rs`.

pub(crate) struct ParsedLine {
    pub segments: Vec<Vec<String>>,
    pub nested: Vec<String>,
}

pub(crate) fn split_shell_segments_checked(line: &str) -> Result<ParsedLine, &'static str> {
    let mut segs = Vec::new();
    let mut nested = Vec::new();
    let mut seg = Vec::new();
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
    let flush_seg = |segs: &mut Vec<Vec<String>>, seg: &mut Vec<String>| {
        if !seg.is_empty() {
            segs.push(std::mem::take(seg));
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
            '\n' | ';' | '(' | ')' => {
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut seg);
            }
            '&' | '|' => {
                if chars.peek() == Some(&c) {
                    chars.next();
                }
                flush_word(&mut seg, &mut word, &mut has_word);
                flush_seg(&mut segs, &mut seg);
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
    if escape || in_single || in_double {
        return Err("unparseable shell syntax");
    }
    flush_word(&mut seg, &mut word, &mut has_word);
    flush_seg(&mut segs, &mut seg);
    Ok(ParsedLine {
        segments: segs,
        nested,
    })
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
