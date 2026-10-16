//! R49 composer utility bar: one state, one production test.
//!
//! Every case mounts the real [`ConversationStream`] in a window and reads the
//! rendered frame's debug bounds — no test-only seam decides any branch. The
//! project rows come from the same `vega_store::projects` query the sidebar's
//! project block uses.
