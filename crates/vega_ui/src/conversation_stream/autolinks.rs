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

#[cfg(test)]
mod tests {
    use super::http_ranges;

    fn targets(text: &str) -> Vec<&str> {
        http_ranges(text)
            .into_iter()
            .map(|range| &text[range])
            .collect()
    }

    #[test]
    fn issue455_http_ranges_keep_exact_url_bytes_and_utf8_boundaries() {
        let text = "查看 https://example.com/中文🙂?q=%E4%B8%AD&next=a,b#部分 和 HTTP://localhost:8080/a(b[c]{d}) 以及 https://[2001:db8::1]:8443/路径?q=1#尾";
        assert_eq!(
            targets(text),
            vec![
                "https://example.com/中文🙂?q=%E4%B8%AD&next=a,b#部分",
                "HTTP://localhost:8080/a(b[c]{d})",
                "https://[2001:db8::1]:8443/路径?q=1#尾",
            ]
        );
        for range in http_ranges(text) {
            assert!(text.is_char_boundary(range.start));
            assert!(text.is_char_boundary(range.end));
        }
    }

    #[test]
    fn issue455_http_ranges_trim_sentence_punctuation_and_unpaired_closers() {
        for (text, target) in [
            ("(https://example.com/a(b)).", "https://example.com/a(b)"),
            ("[https://example.com/a[b]]", "https://example.com/a[b]"),
            ("{https://example.com/a{b}}", "https://example.com/a{b}"),
            ("https://example.com/path.,!?;:", "https://example.com/path"),
            (
                "“https://example.com/中文”，结束",
                "https://example.com/中文",
            ),
            ("'https://example.com/a'", "https://example.com/a"),
            ("<https://example.com/a>", "https://example.com/a"),
            ("https://[::1]:8080/a).", "https://[::1]:8080/a"),
            (
                "https://example.com/（路径））",
                "https://example.com/（路径）",
            ),
        ] {
            assert_eq!(targets(text), vec![target], "{text}");
        }
        assert_eq!(
            targets("https://one.example，http://two.example。https://three.example！"),
            vec![
                "https://one.example",
                "http://two.example",
                "https://three.example"
            ]
        );
    }

    #[test]
    fn issue455_http_ranges_reject_non_candidates_and_word_embedded_schemes() {
        for text in [
            "",
            "plain 中文🙂 text",
            "http://",
            "https://?q=empty",
            "https:///relative",
            "http:/example.com",
            "https:example.com",
            "www.example.com user@example.com /relative //example.com",
            "ftp://example.com file:///tmp/example javascript:alert(1) data:text/plain,hello",
            "abchttps://example.com xHTTP://example.com _https://example.com",
            "user@https://example.com /https://example.com",
        ] {
            assert!(http_ranges(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn issue455_http_ranges_stop_at_control_quotes_and_sentence_boundaries() {
        let text = "https://a.example\nhttps://b.example\tHTTPS://c.example\0https://d.example\"https://e.example；https://f.example：https://g.example、https://h.example";
        assert_eq!(
            targets(text),
            vec![
                "https://a.example",
                "https://b.example",
                "HTTPS://c.example",
                "https://d.example",
                "https://e.example",
                "https://f.example",
                "https://g.example",
                "https://h.example",
            ]
        );
    }
}
