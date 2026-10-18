use std::ops::Range;

use super::links::allowed_link;
use super::model::{SpanStyle, StreamSpan};

pub(super) fn text_spans(text: &str) -> Vec<StreamSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    for range in http_ranges(text) {
        if cursor < range.start {
            spans.push(StreamSpan {
                text: text[cursor..range.start].to_owned(),
                style: SpanStyle::Plain,
                link: None,
            });
        }
        let target = text[range.clone()].to_owned();
        spans.push(StreamSpan {
            text: target.clone(),
            style: SpanStyle::Link,
            link: Some(target),
        });
        cursor = range.end;
    }
    if cursor < text.len() {
        spans.push(StreamSpan {
            text: text[cursor..].to_owned(),
            style: SpanStyle::Plain,
            link: None,
        });
    }
    spans
}

fn http_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    if !text.bytes().any(|byte| matches!(byte, b'h' | b'H')) {
        return ranges;
    }
    let mut chars = text.char_indices().peekable();
    let mut previous = None;
    while let Some((start, ch)) = chars.next() {
        let boundary = previous
            .is_none_or(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '@' | '/'));
        previous = Some(ch);
        if !boundary
            || !matches!(ch, 'h' | 'H')
            || !["http://", "https://"].iter().any(|scheme| {
                text.get(start..start + scheme.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
            })
        {
            continue;
        }
        let mut end = start + ch.len_utf8();
        let mut balance = [0_isize; 5];
        while let Some(&(index, ch)) = chars.peek() {
            if url_boundary(ch) {
                break;
            }
            let _ = chars.next();
            previous = Some(ch);
            if let Some((index, opening)) = bracket(ch) {
                balance[index] += if opening { 1 } else { -1 };
            }
            end = index + ch.len_utf8();
        }
        while let Some(ch) = text[start..end].chars().next_back() {
            if matches!(ch, '.' | ',' | '!' | '?' | ';' | ':') {
                end -= ch.len_utf8();
            } else if let Some((index, false)) = bracket(ch)
                && balance[index] < 0
            {
                balance[index] += 1;
                end -= ch.len_utf8();
            } else {
                break;
            }
        }
        if allowed_link(&text[start..end]) {
            ranges.push(start..end);
        }
    }
    ranges
}

fn url_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            '<' | '>'
                | '"'
                | '\''
                | '`'
                | '‘'
                | '’'
                | '“'
                | '”'
                | '«'
                | '»'
                | '‹'
                | '›'
                | '「'
                | '」'
                | '『'
                | '』'
                | '《'
                | '》'
                | '，'
                | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | '…'
        )
}

fn bracket(ch: char) -> Option<(usize, bool)> {
    match ch {
        '(' => Some((0, true)),
        ')' => Some((0, false)),
        '[' => Some((1, true)),
        ']' => Some((1, false)),
        '{' => Some((2, true)),
        '}' => Some((2, false)),
        '（' => Some((3, true)),
        '）' => Some((3, false)),
        '【' => Some((4, true)),
        '】' => Some((4, false)),
        _ => None,
    }
}
