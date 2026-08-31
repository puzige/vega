use super::*;

/// Integer millionths of one US dollar.
/// Integer millionths of one US dollar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Microcents(pub i64);

/// Fixed upper bound for the provisional visible-output character counter
/// (S7-T39/C3). The estimate is bounded by construction: additional deltas
/// past the cap are not counted, so `ceil(chars/4)` can never exceed
/// `METER_PROVISIONAL_CHAR_CAP / 4`.
pub const METER_PROVISIONAL_CHAR_CAP: u64 = 1 << 32;

/// Frozen per-run output estimator backing the `≈US$` provisional counter
/// segment (S7-T39/C3/C4). Built once at run start from the immutable pricing
/// selection; the estimate reuses the exact integer quote engine, so no cost
/// formula ever leaves `vega_token`/`vega_conversation`.
pub struct RunUsageEstimator {
    model: String,
    catalog: vega_token::PricingCatalog,
    started_utc_seconds: i64,
}

impl std::fmt::Debug for RunUsageEstimator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunUsageEstimator")
            .field("model", &self.model)
            .field("catalog_entries", &self.catalog.specs().len())
            .field("started_utc_seconds", &self.started_utc_seconds)
            .finish()
    }
}

impl RunUsageEstimator {
    /// Freezes the estimator for one run at the current Unix UTC time.
    /// Returns `None` when the monotonic clock is unavailable or the model is
    /// not priced by the given catalog (the provisional cost then displays
    /// `—` instead of a fabricated value).
    pub fn new(model: &str, catalog: vega_token::PricingCatalog) -> Option<Self> {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        // Zero-output quote validates exact catalog membership without cost.
        catalog
            .quote(
                model,
                vega_token::UsageCounts {
                    input: 0,
                    output: 0,
                    cache_read: 0,
                    cache_write: 0,
                },
                seconds,
            )
            .ok()?;
        Some(Self {
            model: model.to_string(),
            catalog,
            started_utc_seconds: seconds,
        })
    }

    /// Quotes the provisional cost of `output_tokens` estimated output tokens
    /// with the frozen selection and timestamp.
    fn estimate_output_cost(&self, output_tokens: u64) -> Option<Microcents> {
        let quote = self
            .catalog
            .quote(
                &self.model,
                vega_token::UsageCounts {
                    input: 0,
                    output: output_tokens,
                    cache_read: 0,
                    cache_write: 0,
                },
                self.started_utc_seconds,
            )
            .ok()?;
        Some(Microcents(quote.cost_microcents))
    }
}

/// Bounded compact meter reading rendered by the Composer counter
/// (S7-T39/C4). `cost: None` displays `—`; `available: false` fails the whole
/// counter closed as `—` after a checked-overflow latch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterSnapshot {
    /// Cumulative usage tokens (all four fields) plus any provisional
    /// estimate while streaming.
    pub tokens: u64,
    /// Cumulative calibrated cost, or the provisional estimate while
    /// streaming; `None` displays `—`.
    pub cost: Option<Microcents>,
    /// `true` while the display contains a provisional estimate (`≈` prefix).
    pub provisional: bool,
    /// `false` after a checked-overflow latch: the whole counter displays `—`
    /// instead of a partially trustworthy number.
    pub available: bool,
}

impl MeterSnapshot {
    /// Compact single-line counter text (C4): `12.4k tok · US$0.12`,
    /// `≈3 tok · ≈US$0.001` while streaming, `0 tok · —` when unpriced.
    pub fn display(&self) -> String {
        if !self.available {
            return "—".to_string();
        }
        let prefix = if self.provisional { "≈" } else { "" };
        let tokens = format_compact_tokens(self.tokens);
        match self.cost {
            Some(cost) => {
                format!("{prefix}{tokens} tok · {prefix}US${}", format_usd(cost.0))
            }
            None => format!("{prefix}{tokens} tok · —"),
        }
    }
}

pub(crate) fn format_compact_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        format!("{tokens}")
    } else if tokens < 1_000_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    }
}

/// Formats microcents as a USD decimal with trailing zeros trimmed, keeping
/// enough precision to distinguish non-zero microcents (C4). Priced zero is a
/// real value (`0`); unknown cost never reaches this formatter (`—`).
pub(crate) fn format_usd(microcents: i64) -> String {
    if microcents < 0 {
        return "—".to_string();
    }
    let dollars = microcents / 1_000_000;
    let fraction = (microcents % 1_000_000) as u64;
    if fraction == 0 {
        return format!("{dollars}");
    }
    let mut digits = format!("{fraction:0>6}");
    while digits.ends_with('0') {
        digits.pop();
    }
    format!("{dollars}.{digits}")
}

/// Durable calibrated baseline restored after a restart (S7-T39/C4): the
/// checked `vega_store::token_usage` aggregate projected into meter space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoredUsage {
    /// Sum of all four usage token fields across the thread's priced rows.
    pub tokens: u64,
    /// Checked cost total, or `None` when any row is legacy/unpriced or the
    /// aggregate failed closed.
    pub cost: Option<Microcents>,
}

/// In-memory thread meter projection (S7-T39/C3/C4): provisional
/// visible-output estimates per provider call, in-place calibration when the
/// authoritative `UsageUpdated` arrives, and a fail-closed cumulative total.
///
/// The meter is a pure projection over the shared event stream: no IO, no
/// persistence, no logging; estimates never write `token_usage`. All
/// arithmetic is checked with fixed caps; overflow latches the whole counter
/// to `—` instead of displaying a wrong number.
#[derive(Debug, Default)]
pub struct ConversationMeter {
    calibrated_tokens: u64,
    calibrated_cost: Option<i64>,
    /// Latched: at least one unpriced usage row was calibrated, so mixed
    /// totals fail closed (C5) and the cost segment stays `—` forever.
    unpriced_seen: bool,
    /// Latched: checked overflow; the whole counter displays `—`.
    degraded: bool,
    /// A task is between `MessageStarted` and its terminal event.
    in_run: bool,
    /// Visible Unicode scalars accumulated for the current provider call.
    round_chars: u64,
    /// `true` once the current round has seen a non-empty visible delta.
    round_visible: bool,
    estimator: Option<RunUsageEstimator>,
}

impl ConversationMeter {
    /// Installs the frozen per-run estimator (run-start ownership handoff).
    pub fn install_run_estimator(&mut self, estimator: Option<RunUsageEstimator>) {
        self.estimator = estimator;
    }

    /// Restores the calibrated baseline from the durable aggregate after a
    /// restart (C4). An unavailable cost latches the unpriced fail-closed
    /// state so later priced usage cannot resurrect a partial total.
    pub fn restore(&mut self, usage: RestoredUsage) {
        self.calibrated_tokens = usage.tokens;
        self.calibrated_cost = usage.cost.map(|cost| cost.0);
        if usage.cost.is_none() {
            self.unpriced_seen = true;
        }
    }

    /// Clears run-scoped state when a run ends outside the event path
    /// (spawn failure, controller error).
    pub fn end_run(&mut self) {
        self.in_run = false;
        self.round_chars = 0;
        self.round_visible = false;
    }

    /// Applies one conversation event; returns whether the snapshot changed.
    /// Empty deltas and non-visible streams produce no noise (`false`).
    pub fn apply(&mut self, event: &ConversationEvent) -> bool {
        match event {
            ConversationEvent::MessageStarted { .. } => {
                self.round_chars = 0;
                self.round_visible = false;
                self.in_run = true;
                false
            }
            ConversationEvent::TextDelta { delta, .. } => {
                if !self.in_run {
                    return false;
                }
                let count = delta.chars().count() as u64;
                if count == 0 {
                    return false;
                }
                let remaining = METER_PROVISIONAL_CHAR_CAP.saturating_sub(self.round_chars);
                self.round_chars = self.round_chars.saturating_add(count.min(remaining));
                self.round_visible = true;
                true
            }
            // C3: thinking/reasoning is not visible output and stays out of
            // the estimate.
            ConversationEvent::ThinkingDelta { .. } => false,
            ConversationEvent::ToolCallProposed { .. } => {
                if !self.in_run {
                    return false;
                }
                // C3: a round without usage ends at the first tool boundary so
                // the next provider call estimates from zero.
                let changed = self.round_visible;
                self.round_chars = 0;
                self.round_visible = false;
                changed
            }
            ConversationEvent::UsageUpdated {
                usage,
                cost,
                pricing,
                ..
            } => {
                if !self.in_run {
                    // Late/duplicate usage after the run terminal: ignore.
                    return false;
                }
                self.round_chars = 0;
                self.round_visible = false;
                let call_tokens = usage
                    .input
                    .checked_add(usage.output)
                    .and_then(|total| total.checked_add(usage.cache_read))
                    .and_then(|total| total.checked_add(usage.cache_write));
                let Some(call_tokens) = call_tokens else {
                    self.degrade();
                    return true;
                };
                let Some(tokens) = self.calibrated_tokens.checked_add(call_tokens) else {
                    self.degrade();
                    return true;
                };
                self.calibrated_tokens = tokens;
                match pricing {
                    None => {
                        self.unpriced_seen = true;
                        self.calibrated_cost = None;
                    }
                    Some(_) => {
                        if !self.unpriced_seen {
                            self.calibrated_cost = match self.calibrated_cost {
                                Some(total) => total.checked_add(cost.0),
                                None => Some(cost.0),
                            };
                            if self.calibrated_cost.is_none() {
                                self.degrade();
                                return true;
                            }
                        }
                    }
                }
                true
            }
            ConversationEvent::MessageFinished { .. }
            | ConversationEvent::Interrupted { .. }
            | ConversationEvent::Error { .. } => {
                let changed = self.round_visible;
                self.end_run();
                changed
            }
            // Tool progress cannot move the visible-output estimate.
            ConversationEvent::ToolCallApproved { .. }
            | ConversationEvent::ToolCallOutput { .. }
            | ConversationEvent::ToolCallFinished { .. } => false,
        }
    }

    fn degrade(&mut self) {
        self.degraded = true;
        self.calibrated_tokens = 0;
        self.calibrated_cost = None;
        self.round_chars = 0;
        self.round_visible = false;
    }

    /// Projects the current counter state.
    pub fn snapshot(&self) -> MeterSnapshot {
        if self.degraded {
            return MeterSnapshot {
                tokens: 0,
                cost: None,
                provisional: false,
                available: false,
            };
        }
        let estimate_tokens = self.round_chars.div_ceil(4);
        let Some(tokens) = self.calibrated_tokens.checked_add(estimate_tokens) else {
            return MeterSnapshot {
                tokens: 0,
                cost: None,
                provisional: false,
                available: false,
            };
        };
        let cost = if self.round_visible {
            match (self.estimator.as_ref(), self.unpriced_seen) {
                // C4: while streaming, the cost segment shows the provisional
                // output estimate on top of the calibrated baseline (which is
                // still zero before the first authoritative usage arrives);
                // the `≈` prefix marks the whole segment approximate.
                (Some(estimator), false) => estimator
                    .estimate_output_cost(estimate_tokens)
                    .and_then(|estimate| self.calibrated_cost.unwrap_or(0).checked_add(estimate.0))
                    .map(Microcents),
                // Unpriced history or an unfrozen model: never fabricate a
                // cost, not even approximately.
                _ => None,
            }
        } else {
            self.calibrated_cost.map(Microcents)
        };
        MeterSnapshot {
            tokens,
            cost,
            provisional: self.round_visible,
            available: true,
        }
    }
}
