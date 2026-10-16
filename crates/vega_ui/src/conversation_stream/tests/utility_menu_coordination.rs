//! C1–C3: production Composer utility-menu coordination.
//!
//! These tests mount the real `ConversationStream`, drive the actual chip
//! mouse/key paths, and inspect the painted branch rows rather than inferring
//! state from the selector model alone.

use super::r64_popup_deferred::{PROJECT_BINDING, bounds, click, install_utility_globals};
use super::*;
use gpui_kit::{Hsla, Modifiers, Quad, Rgba, VisualTestContext};
