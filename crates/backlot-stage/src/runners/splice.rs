//! Byte-exact JSON splicing for files a runner partly owns: the
//! stage replaces one top-level member (`permissions`) without
//! parsing or rewriting the rest — keys it must never read, like a
//! cursor `authInfo`, keep their bytes. Shared by the cursor and cmd
//! adapters; moved here to keep `mod.rs` about the adapter registry.

/// A stage-written file, mode 600 — a runner's config file can end
/// up holding account material once the agent has run.
#[cfg(unix)]
pub(crate) fn write600(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, content)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
pub(crate) fn write600(path: &std::path::Path, content: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, content)
}

/// A JSON string literal in `text` starting at `at` (which must be
/// the opening quote): its end offset, or `None` when unterminated.
pub(crate) fn string_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = at + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

/// The end offset of the JSON value starting at `at` — object,
/// array, string, or a scalar literal; nesting and strings handled.
/// A scalar runs to the next `,` `}` `]` or the text's end.
pub(crate) fn value_end(text: &str, at: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => i = string_end(text, i)?,
            b'{' | b'[' => {
                depth += 1;
                i += 1;
            }
            b'}' | b']' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
                i += 1;
            }
            b',' if depth == 0 => return Some(i),
            _ => i += 1,
        }
        if depth == 0 && i > at && matches!(bytes[at], b'{' | b'[' | b'"') {
            // A delimited value ended at its own close.
            return Some(i);
        }
    }
    if depth == 0 && i > at { Some(i) } else { None }
}

/// `text`'s top-level `"permissions"` value replaced by `value`,
/// spliced byte-exact so every other key — an `authInfo` included,
/// never parsed here — keeps its bytes. When the object has no
/// `permissions` key the member is appended; a text that is not a
/// JSON object at all yields `None` and the caller writes fresh.
pub(crate) fn splice_permissions(text: &str, value: &str) -> Option<String> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&b'{') {
        return None;
    }
    let mut members: Vec<(usize, usize, usize)> = Vec::new(); // (key start, value start, value end)
    let mut i = 1;
    let mut close = None;
    while i < bytes.len() {
        i += text[i..]
            .bytes()
            .take_while(|b| b.is_ascii_whitespace() || *b == b',')
            .count();
        match bytes.get(i) {
            Some(&b'}') => {
                close = Some(i);
                break;
            }
            Some(&b'"') => {
                let key_start = i;
                i = string_end(text, i)?;
                i += text[i..]
                    .bytes()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                if bytes.get(i) != Some(&b':') {
                    return None;
                }
                i += 1;
                i += text[i..]
                    .bytes()
                    .take_while(|b| b.is_ascii_whitespace())
                    .count();
                let vstart = i;
                i = value_end(text, i)?;
                members.push((key_start, vstart, i));
            }
            _ => return None,
        }
    }
    let close = close?;
    for (ks, vs, ve) in &members {
        if text[*ks..string_end(text, *ks)?] == *"\"permissions\"" {
            return Some(format!("{}{}{}", &text[..*vs], value, &text[*ve..]));
        }
    }
    // No permissions key: append before the closing brace.
    let insertion = if members.is_empty() {
        format!("\n  \"permissions\": {value}\n")
    } else {
        format!(",\n  \"permissions\": {value}\n")
    };
    Some(format!("{}{}{}", &text[..close], insertion, &text[close..]))
}
