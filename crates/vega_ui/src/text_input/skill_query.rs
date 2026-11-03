use super::*;

impl TextInput {
    pub fn trailing_skill_query(&self) -> Option<(Range<usize>, String)> {
        if self.is_composing() || !self.selected_range.is_empty() || self.masked {
            return None;
        }
        let caret = self.cursor_offset();
        let prefix = self.content.get(..caret)?;
        let start = prefix.rfind('$')?;
        let before = &prefix[..start];
        if before
            .chars()
            .next_back()
            .is_some_and(|ch| !ch.is_whitespace())
        {
            return None;
        }
        let query = &prefix[start + 1..];
        if query.is_empty()
            || query.chars().next().is_some_and(char::is_numeric)
            || query.chars().any(|ch| {
                ch.is_whitespace() || ch.is_control() || matches!(ch, '/' | '\\' | ':' | '`' | '$')
            })
            || inside_code(before)
            || self
                .skill_mentions
                .iter()
                .any(|mention| mention.range.start < caret && start < mention.range.end)
            || self
                .mentions
                .iter()
                .any(|mention| mention.range.start < caret && start < mention.range.end)
        {
            return None;
        }
        Some((start..caret, query.to_owned()))
    }
}

fn inside_code(text: &str) -> bool {
    let mut fence: Option<(u8, usize)> = None;
    let mut inline_ticks = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        let marker_len = marker.map_or(0, |marker| {
            trimmed.bytes().take_while(|byte| *byte == marker).count()
        });
        if indent <= 3 && matches!(marker, Some(b'`' | b'~')) && marker_len >= 3 {
            if let Some((opening, count)) = fence {
                if marker == Some(opening)
                    && marker_len >= count
                    && trimmed[marker_len..].trim().is_empty()
                {
                    fence = None;
                }
            } else if let Some(marker) = marker {
                fence = Some((marker, marker_len));
                inline_ticks = 0;
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if !line.ends_with('\n') && (indent >= 4 || line.starts_with('\t')) {
            return true;
        }
        let bytes = line.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'\\' && inline_ticks == 0 {
                index = (index + 2).min(bytes.len());
            } else if bytes[index] == b'`' {
                let count = bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'`')
                    .count();
                if inline_ticks == 0 {
                    inline_ticks = count;
                } else if inline_ticks == count {
                    inline_ticks = 0;
                }
                index += count;
            } else {
                index += 1;
            }
        }
    }
    fence.is_some() || inline_ticks > 0
}
