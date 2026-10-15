use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourcePosition {
    pub source: Range<usize>,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct SourceSpan {
    pub rendered: Range<usize>,
    pub source: Range<usize>,
    pub literal: bool,
    pub offset: usize,
}

impl SourceSpan {
    pub fn position(&self, byte: usize) -> SourcePosition {
        let offset = byte - self.rendered.start;
        if self.literal {
            let source = self.source.start + offset;
            SourcePosition {
                source: source..source,
                offset: 0,
            }
        } else {
            SourcePosition {
                source: self.source.clone(),
                offset: self.offset + offset,
            }
        }
    }

    pub fn rendered_position(&self, position: &SourcePosition) -> Option<usize> {
        if self.literal
            && position.source.is_empty()
            && position.source.start >= self.source.start
            && position.source.start <= self.source.end
        {
            Some(self.rendered.start + position.source.start - self.source.start)
        } else if !self.literal
            && position.source == self.source
            && position.offset >= self.offset
            && position.offset <= self.offset + self.rendered.len()
        {
            Some(self.rendered.start + position.offset - self.offset)
        } else {
            None
        }
    }
}

pub struct SourceTextMap {
    text: String,
    spans: Vec<SourceSpan>,
    cursor: usize,
    next_span: usize,
    aligned: bool,
}

impl SourceTextMap {
    pub fn new(source: &str) -> Self {
        let mut stream = mdstream::MdStream::new(mdstream::Options {
            reference_definitions: mdstream::ReferenceDefinitionsMode::Invalidate,
            ..Default::default()
        });
        let update = stream.append(source);
        let completion = update
            .pending
            .as_ref()
            .and_then(|pending| {
                source
                    .ends_with(&pending.raw)
                    .then(|| pending.display.as_deref()?.strip_prefix(&pending.raw))
                    .flatten()
            })
            .unwrap_or_default();
        let repaired = format!("{source}{completion}");
        let options =
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        let mut map = Self {
            text: String::new(),
            spans: Vec::new(),
            cursor: 0,
            next_span: 0,
            aligned: true,
        };
        let mut code = false;
        for (event, mut range) in Parser::new_ext(&repaired, options).into_offset_iter() {
            let value = match event {
                Event::Start(Tag::CodeBlock(_)) => {
                    code = true;
                    continue;
                }
                Event::End(TagEnd::CodeBlock) => {
                    code = false;
                    continue;
                }
                Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                    text.into_string()
                }
                Event::Code(text) => {
                    let raw = &repaired[range.clone()];
                    let ticks = raw.bytes().take_while(|byte| *byte == b'`').count();
                    if ticks > 0 && raw.len() >= ticks * 2 {
                        range = range.start + ticks..range.end - ticks;
                        let inner = &repaired[range.clone()];
                        if inner.starts_with(' ')
                            && inner.ends_with(' ')
                            && inner.chars().any(|ch| ch != ' ')
                        {
                            range = range.start + 1..range.end - 1;
                        }
                    }
                    text.into_string()
                }
                Event::SoftBreak => " ".into(),
                Event::HardBreak => "\n".into(),
                _ => continue,
            };
            let literal = repaired.get(range.clone()) == Some(value.as_str());
            if code && literal {
                let mut offset = 0;
                for line in value.split_inclusive('\n') {
                    let text = line.strip_suffix('\n').unwrap_or(line);
                    map.push(
                        text,
                        range.start + offset..range.start + offset + text.len(),
                        true,
                        source.len(),
                    );
                    offset += line.len();
                }
            } else {
                map.push(&value, range, literal, source.len());
            }
        }
        map
    }

    fn push(&mut self, text: &str, source: Range<usize>, literal: bool, source_len: usize) {
        let start = self.text.len();
        self.text.push_str(text);
        if !text.is_empty() && source.end <= source_len {
            self.spans.push(SourceSpan {
                rendered: start..self.text.len(),
                source,
                literal,
                offset: 0,
            });
        }
    }

    pub fn take(&mut self, text: &str, rendered: usize) -> Vec<SourceSpan> {
        if !self.aligned || !self.text[self.cursor..].starts_with(text) {
            self.aligned = false;
            return Vec::new();
        }
        let end = self.cursor + text.len();
        let mut spans = Vec::new();
        while let Some(span) = self.spans.get(self.next_span) {
            if span.rendered.start >= end {
                break;
            }
            let start = span.rendered.start.max(self.cursor);
            let to = span.rendered.end.min(end);
            if start < to {
                let mut mapped = span.clone();
                mapped.rendered = rendered + start - self.cursor..rendered + to - self.cursor;
                if mapped.literal {
                    mapped.source = span.source.start + start - span.rendered.start
                        ..span.source.start + to - span.rendered.start;
                } else {
                    mapped.offset += start - span.rendered.start;
                }
                spans.push(mapped);
            }
            if span.rendered.end > end {
                break;
            }
            self.next_span += 1;
        }
        self.cursor = end;
        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_positions_distinguish_repeated_markdown_and_survive_append() {
        let source = "**same**\n\n**same** with &amp; `code`";
        let mut map = SourceTextMap::new(source);
        let first = map.take("same", 0);
        let second = map.take("same with & code", 6);
        assert_ne!(first[0].position(0), second[0].position(6));
        assert_eq!(second[0].position(6).source.start, 12);
        let mut newer = SourceTextMap::new(&format!("{source} extra"));
        newer.take("same", 0);
        let newer = newer.take("same with & code extra", 6);
        assert_eq!(second[0].position(6), newer[0].position(6));
        assert!(second.iter().any(|span| !span.literal));
    }
}
