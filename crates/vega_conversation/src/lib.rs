//! Thread and message models, streaming state machine, and context assembly.
//!
//! T11 (A1-02) scope: the `types` data-model subset and the thread
//! orchestration layer. The streaming state machine and context assembly
//! land in S3/S4.

pub mod agent;
pub mod threads;
pub mod types;
