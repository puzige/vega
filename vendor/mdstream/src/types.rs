use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u64);

impl fmt::Debug for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockId({})", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    Committed,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading,
    ThematicBreak,
    CodeFence,
    List,
    BlockQuote,
    Table,
    HtmlBlock,
    MathBlock,
    FootnoteDefinition,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: BlockId,
    pub status: BlockStatus,
    pub kind: BlockKind,
    pub raw: String,

    pub display: Option<String>,
}

impl Block {
    pub fn display_or_raw(&self) -> &str {
        self.display.as_deref().unwrap_or(&self.raw)
    }

    pub fn code_fence_header(&self) -> Option<crate::syntax::CodeFenceHeader<'_>> {
        if self.kind != BlockKind::CodeFence {
            return None;
        }
        crate::syntax::parse_code_fence_header_from_block(&self.raw)
    }

    pub fn code_fence_language(&self) -> Option<&str> {
        self.code_fence_header().and_then(|h| h.language)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingBlockRef<'a> {
    pub id: BlockId,
    pub kind: BlockKind,
    pub raw: &'a str,

    pub display: Option<&'a str>,
}

impl<'a> PendingBlockRef<'a> {
    pub fn display_or_raw(&self) -> &'a str {
        self.display.unwrap_or(self.raw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub committed: Vec<Block>,
    pub pending: Option<Block>,

    pub reset: bool,

    pub invalidated: Vec<BlockId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedUpdate {
    pub reset: bool,
    pub invalidated: Vec<BlockId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateRef<'a> {
    pub committed: &'a [Block],
    pub pending: Option<PendingBlockRef<'a>>,
    pub reset: bool,
    pub invalidated: Vec<BlockId>,
}

impl<'a> UpdateRef<'a> {
    pub fn is_empty(&self) -> bool {
        self.committed.is_empty()
            && self.pending.is_none()
            && !self.reset
            && self.invalidated.is_empty()
    }

    pub fn to_owned(&self) -> Update {
        Update {
            committed: self.committed.to_vec(),
            pending: self.pending.as_ref().map(|p| Block {
                id: p.id,
                status: BlockStatus::Pending,
                kind: p.kind,
                raw: p.raw.to_string(),
                display: p.display.map(|d| d.to_string()),
            }),
            reset: self.reset,
            invalidated: self.invalidated.clone(),
        }
    }
}

impl Update {
    pub fn empty() -> Self {
        Self {
            committed: Vec::new(),
            pending: None,
            reset: false,
            invalidated: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.committed.is_empty()
            && self.pending.is_none()
            && !self.reset
            && self.invalidated.is_empty()
    }

    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.committed.iter().chain(self.pending.iter())
    }

    pub fn apply_to(
        self,
        committed: &mut Vec<Block>,
        pending: &mut Option<Block>,
    ) -> AppliedUpdate {
        if self.reset {
            committed.clear();
            *pending = None;
        }
        committed.extend(self.committed);
        *pending = self.pending;
        AppliedUpdate {
            reset: self.reset,
            invalidated: self.invalidated,
        }
    }
}
