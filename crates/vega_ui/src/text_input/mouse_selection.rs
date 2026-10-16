use std::ops::Range;

#[derive(Clone, Copy)]
pub(super) enum SelectionGranularity {
    Character,
    Word,
    Line,
}

impl SelectionGranularity {
    pub(super) fn range(self, text: &str, offset: usize) -> Range<usize> {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        match self {
            Self::Character => offset..offset,
            Self::Line => {
                let start = text[..offset].rfind('\n').map_or(0, |newline| newline + 1);
                let end = text[offset..]
                    .find('\n')
                    .map_or(text.len(), |newline| offset + newline);
                start..end
            }
            Self::Word => {
                if offset == text.len() {
                    offset = text
                        .char_indices()
                        .next_back()
                        .map_or(0, |(index, _)| index);
                }
                let Some(character) = text[offset..].chars().next() else {
                    return offset..offset;
                };
                if matches!(character, '\n' | '\r') {
                    return offset..offset;
                }
                let kind = selection_character(character);
                let connects =
                    |next| kind != SelectionCharacter::Other && selection_character(next) == kind;
                let start = text[..offset]
                    .chars()
                    .rev()
                    .take_while(|character| connects(*character))
                    .fold(offset, |start, character| start - character.len_utf8());
                let end = text[offset + character.len_utf8()..]
                    .chars()
                    .take_while(|character| connects(*character))
                    .fold(offset + character.len_utf8(), |end, character| {
                        end + character.len_utf8()
                    });
                start..end
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct MouseSelection {
    pub(super) anchor: Range<usize>,
    pub(super) granularity: SelectionGranularity,
    pub(super) content_revision: u64,
}

impl MouseSelection {
    pub(super) fn extend(&self, unit: Range<usize>) -> (Range<usize>, bool) {
        if unit.start < self.anchor.start {
            (unit.start..self.anchor.end, true)
        } else if unit.end > self.anchor.end {
            (self.anchor.start..unit.end, false)
        } else {
            (self.anchor.clone(), false)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionCharacter {
    Word,
    Whitespace,
    Other,
}

fn selection_character(character: char) -> SelectionCharacter {
    if character == '_'
        || character.is_ascii_alphanumeric()
        || matches!(character, '\u{00C0}'..='\u{024F}' | '\u{0400}'..='\u{04FF}' | '\u{1E00}'..='\u{1EFF}' | '\u{0300}'..='\u{036F}')
    {
        SelectionCharacter::Word
    } else if character.is_whitespace() && !matches!(character, '\n' | '\r') {
        SelectionCharacter::Whitespace
    } else {
        SelectionCharacter::Other
    }
}
