//! Core shared types (tech-spec §3): the T11 data-model subset only.
//!
//! This card deliberately ships the *Thread* structure plus the
//! [`ThreadMode`]/[`ThreadStatus`] enums, aligned field-by-field with the
//! `threads` DDL (`migrations/0001_init.sql`). The streaming/event payload
//! types (runtime events, chat messages, tool calls) belong to S3/S4 and
//! must not appear here yet.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};

/// Stable, content-free pricing settings failure vocabulary.
mod artifact;
mod events;
mod meter;
mod permission;
mod pricing;
mod thread;
mod tool_calls;
mod usage;
mod workspace;

#[cfg(test)]
mod tests_audit_wire;
#[cfg(test)]
mod tests_conversion;

pub use artifact::*;
pub use events::*;
pub use meter::*;
pub use permission::*;
pub use pricing::*;
pub use thread::*;
pub use tool_calls::*;
pub use usage::*;
pub use workspace::*;
