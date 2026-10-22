use crate::pending::TerminatorOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootnotesMode {
    SingleBlock,

    Invalidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceDefinitionsMode {
    StabilityFirst,

    Invalidate,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub footnotes: FootnotesMode,
    pub reference_definitions: ReferenceDefinitionsMode,
    pub terminator: TerminatorOptions,
    pub terminator_window_bytes: usize,

    pub max_buffer_bytes: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            footnotes: FootnotesMode::SingleBlock,
            reference_definitions: ReferenceDefinitionsMode::StabilityFirst,
            terminator: TerminatorOptions::default(),
            terminator_window_bytes: 16 * 1024,
            max_buffer_bytes: None,
        }
    }
}
