// Derived from herdr src/client/clipboard_images.rs (Apache-2.0).
//! Dropped-image-path parsing for the remote image-paste bridge (R9): a
//! bracketed paste holding exactly one local absolute image path is
//! what a terminal file drop delivers. Reading the file back is bounded
//! by the `terminal.put` limit, like the clipboard reads.

use std::path::{Path, PathBuf};

use crate::clipboard::{ClipboardImage, LimitedRead, MAX_CLIPBOARD_IMAGE_BYTES, read_limited};

/// A pasted text holding exactly one local absolute image path — what a
/// terminal file drop delivers. Quoted and backslash-escaped forms are
/// normalized first, like the reference's drop handling.
pub(crate) fn image_path_from_paste(text: &str) -> Option<(PathBuf, &'static str)> {
    let text = normalized_drop_text(text)?;
    let text = unescape_drop_path(strip_matching_path_quotes(text));
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return None;
    }
    let extension = recognized_image_extension(path.extension()?.to_str()?)?;
    Some((path, extension))
}

fn normalized_drop_text(text: &str) -> Option<&str> {
    let text = text.trim_end_matches(['\r', '\n']);
    (!text.is_empty() && !text.contains(['\r', '\n'])).then_some(text)
}

fn strip_matching_path_quotes(text: &str) -> &str {
    if text.len() < 2 {
        return text;
    }
    let bytes = text.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(b'\''), Some(b'\'')) | (Some(b'"'), Some(b'"')) => &text[1..text.len() - 1],
        _ => text,
    }
}

fn unescape_drop_path(text: &str) -> String {
    let mut unescaped = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some(escaped) => unescaped.push(escaped),
                None => unescaped.push(ch),
            }
        } else {
            unescaped.push(ch);
        }
    }
    unescaped
}

fn recognized_image_extension(extension: &str) -> Option<&'static str> {
    if extension.eq_ignore_ascii_case("png") {
        Some("png")
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        Some("jpg")
    } else if extension.eq_ignore_ascii_case("gif") {
        Some("gif")
    } else if extension.eq_ignore_ascii_case("webp") {
        Some("webp")
    } else if extension.eq_ignore_ascii_case("bmp") {
        Some("bmp")
    } else {
        None
    }
}

/// The dropped image file: a regular file under the `terminal.put`
/// limit, like the reference's `read_image_file`. The filename's
/// extension is the claimed format — contents are not sniffed.
pub(crate) fn read_image_file(path: &Path, extension: &'static str) -> Option<ClipboardImage> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let bytes =
        match read_limited(std::fs::File::open(path).ok()?, MAX_CLIPBOARD_IMAGE_BYTES).ok()? {
            LimitedRead::Complete(bytes) => bytes,
            LimitedRead::Empty | LimitedRead::Oversized => return None,
        };
    Some(ClipboardImage { bytes, extension })
}
