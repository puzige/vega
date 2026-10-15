use super::THINKING_DELTA_BYTES;

#[derive(Clone, Default)]
pub(super) struct SourceWindow {
    pub(super) start: usize,
    fence: Option<(u8, usize, String)>,
    line: String,
    line_overflow: bool,
}

impl SourceWindow {
    pub(super) fn prefix(&self) -> String {
        self.fence
            .as_ref()
            .map_or_else(String::new, |(marker, count, language)| {
                format!(
                    "{}{}\n",
                    (*marker as char).to_string().repeat(*count),
                    language
                )
            })
    }

    fn discard(&mut self, source: &str) {
        self.start += source.len();
        for part in source.split_inclusive('\n') {
            let room = 1024usize.saturating_sub(self.line.len());
            let mut end = room.min(part.len());
            while !part.is_char_boundary(end) {
                end -= 1;
            }
            self.line.push_str(&part[..end]);
            self.line_overflow |= end < part.len();
            if part.ends_with('\n') {
                let line = self.line.trim_end_matches(['\n', '\r']);
                let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
                if spaces <= 3 {
                    let line = &line[spaces..];
                    if let Some(marker @ (b'`' | b'~')) = line.as_bytes().first().copied() {
                        let count = line.bytes().take_while(|byte| *byte == marker).count();
                        if count >= 3 {
                            if let Some((open, length, _)) = &self.fence {
                                if *open == marker
                                    && count >= *length
                                    && !self.line_overflow
                                    && line[count..].trim().is_empty()
                                {
                                    self.fence = None;
                                }
                            } else if marker != b'`' || !line[count..].contains('`') {
                                self.fence = Some((
                                    marker,
                                    count,
                                    line[count..].trim().chars().take(80).collect(),
                                ));
                            }
                        }
                    }
                }
                self.line.clear();
                self.line_overflow = false;
            }
        }
    }

    pub(super) fn append(&mut self, text: &mut String, delta: &str, limit: usize) -> bool {
        let before = self.start;
        let delta = if delta.len() > limit {
            self.discard(text);
            text.clear();
            let mut cut = delta.len() - limit;
            while !delta.is_char_boundary(cut) {
                cut += 1;
            }
            cut = paragraph_boundary(delta, cut);
            self.discard(&delta[..cut]);
            &delta[cut..]
        } else {
            delta
        };
        text.push_str(delta);
        if text.len() > limit {
            let mut cut = (text.len() - limit).max(THINKING_DELTA_BYTES.min(text.len()));
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            cut = paragraph_boundary(text, cut);
            self.discard(&text[..cut]);
            text.drain(..cut);
        }
        self.start != before
    }

    pub(super) fn clear(&mut self, text: &mut String) {
        self.discard(text);
        *text = String::new();
    }
}

fn paragraph_boundary(text: &str, cut: usize) -> usize {
    let start = text[..cut].rfind('\n').map_or(0, |index| index + 1);
    let line = text[start..].trim_start_matches(' ');
    if (line.starts_with("```") || line.starts_with("~~~"))
        && let Some(end) = text[cut..].find('\n').filter(|end| *end < 1024)
    {
        return cut + end + 1;
    }
    let mut end = (cut + THINKING_DELTA_BYTES / 4).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[cut..end]
        .find("\n\n")
        .map_or(cut, |offset| cut + offset + 2)
}
