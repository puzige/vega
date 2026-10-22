use crate::syntax::facts::{fence_end, fence_start};
use std::ops::Range;

#[derive(Debug, Clone, Copy)]
enum Container {
    Quote,
    List { indent: usize },
}

#[derive(Debug, Clone)]
struct OwnedFence {
    character: char,
    length: usize,
    owners: Vec<Container>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FenceContext {
    containers: Vec<Container>,
    active: Option<OwnedFence>,
    protected: Vec<Range<usize>>,
}

impl FenceContext {
    pub(crate) fn protects_line(&self, line: &str) -> bool {
        if self
            .active
            .as_ref()
            .is_some_and(|fence| strip_owners(line, &fence.owners).is_some())
        {
            return true;
        }
        let (owners, cursor) = self.content_prefix(line);
        !owners.is_empty() && fence_start(&line[cursor.byte..]).is_some()
    }

    pub(crate) fn observe_line(&mut self, line: &str, start: usize, end: usize, complete: bool) {
        if let Some(fence) = &self.active {
            if let Some(content) = strip_owners(line, &fence.owners) {
                if let Some(range) = self.protected.last_mut() {
                    range.end = end;
                }
                if complete && fence_end(content, fence.character, fence.length) {
                    self.active = None;
                }
                return;
            }
            self.active = None;
        }

        let (owners, cursor) = self.content_prefix(line);
        self.containers = owners;
        if let Some((character, length)) = fence_start(&line[cursor.byte..]) {
            self.active = Some(OwnedFence {
                character,
                length,
                owners: self.containers.clone(),
            });
            self.protected.push(start..end);
        }
    }

    pub(crate) fn protected_ranges(&self) -> &[Range<usize>] {
        &self.protected
    }

    pub(crate) fn rebase(&mut self, removed: usize) {
        for range in &mut self.protected {
            range.start = range.start.saturating_sub(removed);
            range.end = range.end.saturating_sub(removed);
        }
    }

    pub(crate) fn for_text(text: &str) -> Self {
        let mut context = Self::default();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            let end = offset + line.len();
            let complete = line.ends_with('\n');
            context.observe_line(
                line.strip_suffix('\n').unwrap_or(line),
                offset,
                end,
                complete,
            );
            offset = end;
        }
        context
    }

    fn content_prefix(&self, line: &str) -> (Vec<Container>, Cursor) {
        let mut cursor = Cursor::default();
        let mut owners = Vec::new();
        for owner in &self.containers {
            let mut next = cursor;
            if !next.consume_owner(line, *owner) {
                break;
            }
            cursor = next;
            owners.push(*owner);
        }
        loop {
            if let Some(next) = cursor.quote(line) {
                cursor = next;
                owners.push(Container::Quote);
            } else if let Some((next, indent)) = cursor.list_marker(line) {
                cursor = next;
                owners.push(Container::List { indent });
            } else {
                break;
            }
        }
        (owners, cursor)
    }
}

fn strip_owners<'a>(line: &'a str, owners: &[Container]) -> Option<&'a str> {
    let mut cursor = Cursor::default();
    for owner in owners {
        if !cursor.consume_owner(line, *owner) {
            return None;
        }
    }
    Some(&line[cursor.byte..])
}

#[derive(Clone, Copy, Default)]
struct Cursor {
    byte: usize,
    column: usize,
}

impl Cursor {
    fn whitespace(&mut self, line: &str) -> bool {
        match line.as_bytes().get(self.byte) {
            Some(b' ') => self.column += 1,
            Some(b'\t') => self.column += 4 - self.column % 4,
            _ => return false,
        }
        self.byte += 1;
        true
    }

    fn consume_owner(&mut self, line: &str, owner: Container) -> bool {
        match owner {
            Container::Quote => {
                if let Some(next) = self.quote(line) {
                    *self = next;
                    true
                } else {
                    false
                }
            }
            Container::List { indent } => {
                if line[self.byte..].trim().is_empty() {
                    self.byte = line.len();
                    return true;
                }
                let target = self.column + indent;
                while self.column < target {
                    if !self.whitespace(line) {
                        return false;
                    }
                }
                true
            }
        }
    }

    fn quote(self, line: &str) -> Option<Self> {
        let mut cursor = self;
        while cursor.whitespace(line) {}
        if cursor.column - self.column > 3 || line.as_bytes().get(cursor.byte) != Some(&b'>') {
            return None;
        }
        cursor.byte += 1;
        cursor.column += 1;
        cursor.whitespace(line);
        Some(cursor)
    }

    fn list_marker(self, line: &str) -> Option<(Self, usize)> {
        let mut cursor = self;
        while cursor.whitespace(line) {}
        if cursor.column - self.column > 3 {
            return None;
        }
        let bytes = line.as_bytes();
        match bytes.get(cursor.byte)? {
            b'-' | b'+' | b'*' => {
                cursor.byte += 1;
                cursor.column += 1;
            }
            b'0'..=b'9' => {
                let start = cursor.byte;
                while bytes.get(cursor.byte).is_some_and(u8::is_ascii_digit) {
                    cursor.byte += 1;
                    cursor.column += 1;
                }
                if cursor.byte - start > 9 || !matches!(bytes.get(cursor.byte), Some(b'.' | b')')) {
                    return None;
                }
                cursor.byte += 1;
                cursor.column += 1;
            }
            _ => return None,
        }
        let marker_end = cursor;
        if !cursor.whitespace(line) {
            return None;
        }
        let first_space = cursor;
        while cursor.whitespace(line) {}
        if cursor.column - marker_end.column > 4 || cursor.byte == line.len() {
            cursor = first_space;
        }
        Some((cursor, cursor.column - self.column))
    }
}
