//! Two-level model picker: one state machine, one level on screen, anchored to
//! the model trigger.
//!
//! Contract sources: `docs/vega-r59-two-level-model-picker.md` §3 (the
//! two-level drill-down) and `docs/vega-r61-picker-trigger-anchoring.md` §3
//! (the trigger anchoring that replaced R59's composer-column anchoring).
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame — no test-only seam decides any branch, and no synthetic
//! keyboard event is used to prove focus (AGENTS.md: GPUI focus cannot be
//! driven that way; the structural assertions below are the authority).
//!
//! The defects these tests pin:
//! - R59 D1 the slider card floated inside the model list;
//! - R59 D2 the unbounded menu covered the R49 utility bar;
//! - R59 D3 the card's rows collided;
//! - R61 D1 the R59 column anchoring left the card a whole utility-bar height
//!   away from the model button;
//! - R61 D2 the same anchoring right-aligned the card to the 768px container
//!   edge instead of to the trigger.
//!
//! R61 accepts that a trigger-anchored card overlaps the R49 utility bar: the
//! trigger has ~61px of clearance and the R57-frozen card needs 84px, so the
//! overlap is geometric (R61 §1). The tests below assert the **hug** instead of
//! the clearance, and pin the accepted overlap so re-introducing the column
//! anchoring has to re-open the decision (R61 R5).

// ---- R1/A1: the model button shows only the slider ------------------------

// ---- R2/A2: the slider title opens the model list -------------------------

// ---- R3/A3: the two levels are mutually exclusive -------------------------

// ---- R4: two independent layers, never card-inside-card -------------------

// ---- R61 A1: the card hugs the trigger (and overlaps the utility bar) -----

// ---- R6/D3: no row overlap inside the card --------------------------------

// ---- R7: the trigger label while adjusting a tier -------------------------

// ---- R3/A4: what a model pick does ---------------------------------------
