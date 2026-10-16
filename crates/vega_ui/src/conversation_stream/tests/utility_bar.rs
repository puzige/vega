//! R49 composer utility bar: one state, one production test.
//!
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame's debug bounds — no test-only seam decides any branch. The
//! project rows come from the same `vega_store::projects` query the sidebar's
//! project block uses.

use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};

/// The `permission_thread()` fixture's durable project binding. The utility
/// bar's visibility predicate requires the shared selection to match it.
const PROJECT_BINDING: &str = "project-safe-id";
