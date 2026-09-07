# R10 Settings usage data

2026-09-06: User-requested centralized Usage replaces conversation usage clutter.

The conversation controller reads a consistent snapshot of persisted provider-call
usage using a read-only database connection on a blocking worker. UI receives only
bounded aggregates through `vega_conversation::types`, never SQLite rows.

Lifetime totals include rows before the exclusive refresh instant. Activity covers
365 UTC calendar days including today; model trends cover 30 days, permitting a
7-day subset. Each interval is half-open; today excludes rows at/after refresh.
Days are zero-filled. UTC is explicitly labeled (no implicit local/DST conversion).
Input already includes OpenAI cached input: total tokens = input + output; cache
counts are informational subsets and never added again. Costs retain persisted
pricing: only exact pricing_v1 rows count toward the known priced subtotal, and
unpriced/unknown-version call count remains explicit (never zero-price fiction).

Data is streamed with checked integer addition; negative token/cost/timestamp rows
fail closed. Daily and model memory/output are bounded independently of call count.
The first 32 distinct exact model IDs in lexical order in the recent 30-day window
retain individual series; any remainder is explicitly aggregated as Other models
(an optional model identity, not a reserved string). No case folding occurs.

Acceptance: real migrated owned DB -> production controller tests covering UTC
boundaries, zero days, exact model IDs, mixed pricing, refresh, corruption and
arithmetic overflow; formatting and targeted clippy/test, full gates at integration.
