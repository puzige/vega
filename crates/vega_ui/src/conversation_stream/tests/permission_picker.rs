//! R62 §7: the bottom row's permission chip opens a four-row picker.
//!
//! Contract source: `docs/vega-r62-slider-card-three-rows.md` §7 (R7/R8/R9)
//! and §9 (A9/A10/A11). Every case mounts the real [`ConversationStream`] in a
//! window and drives the production click paths — no test-only seam decides
//! any branch.
//!
//! The defect these tests pin: R57 P2b made `⚠ 确认` static text on the
//! reasoning that the reference implementation's `Full access` label was not
//! an affordance. The user's screenshot proved otherwise, and this suite
//! exists so the decision cannot silently flip back.
//!
//! R63 adds the chip's own glyph to the same contract: the chip must show the
//! mode's icon (hand / shield / warning), not a constant warning triangle.
//! That case lives at the bottom of this file.

use super::*;
use gpui_kit::{Modifiers, VisualTestContext};
