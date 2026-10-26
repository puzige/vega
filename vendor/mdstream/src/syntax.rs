pub(crate) mod containers;
pub(crate) mod facts;

use facts::{fence_end, fence_start, is_space_or_tab, strip_up_to_three_leading_spaces};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeFenceHeader<'a> {
    pub fence_char: char,
    pub fence_len: usize,

    pub info: &'a str,

    pub language: Option<&'a str>,
}

pub fn parse_code_fence_header(line: &str) -> Option<CodeFenceHeader<'_>> {
    let s = strip_up_to_three_leading_spaces(line);
    let (fence_char, fence_len) = fence_start(line)?;

    let info = s[fence_len..].trim();
    let language = info
        .split_whitespace()
        .next()
        .and_then(|tok| if tok.is_empty() { None } else { Some(tok) });

    Some(CodeFenceHeader {
        fence_char,
        fence_len,
        info,
        language,
    })
}

pub fn parse_code_fence_header_from_block(text: &str) -> Option<CodeFenceHeader<'_>> {
    let first_line = text.split('\n').next().unwrap_or(text);
    parse_code_fence_header(first_line)
}

pub fn is_code_fence_closing_line(line: &str, fence_char: char, fence_len: usize) -> bool {
    fence_end(line, fence_char, fence_len)
}

pub fn is_list_marker_line_prefix(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() && is_space_or_tab(bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        return false;
    }
    let marker = bytes[i];
    if marker != b'-' && marker != b'*' && marker != b'+' {
        return false;
    }
    i += 1;
    if i >= bytes.len() {
        return false;
    }
    let mut has_ws = false;
    while i < bytes.len() {
        if is_space_or_tab(bytes[i]) {
            has_ws = true;
            i += 1;
            continue;
        }
        return false;
    }
    has_ws
}
