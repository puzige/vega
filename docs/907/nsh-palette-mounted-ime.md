# Mounted command palette IME development regression

## Acceptance fixed before Rust edits

This is #907 implementation work, not independent review or native OS acceptance.
The existing command-palette specification remains authoritative. This increment
mounts the production `CommandPalette`, its production `presentation::render` and
`register_keys`, real NSH Input and application controller. It does not substitute
a fixture keymap, approximate fonts, fabricated input node or synthetic OS input.

Authorized writes are limited to the presentation's feature-gated child-module
wiring, its new `mounted_regressions.rs`, a minimal gated re-export in
`command_palette.rs`, `tests/nsh_palette_integration.rs` and this document.
No production behavior change is authorized. Existing presentation regressions
must remain byte-identical. Parent owns Cargo/feature/target wiring and all builds.
No Cargo, rustc, network or Git mutation is permitted in this writer's execution.

Required observable behavior:

1. Use official GPUI `TestAppContext`/`TestDispatcher`, `allow_parking`, and the
   current platform's real text system. Mount a root that renders the actual
   palette. Find its unique real Input by bounded AppView edit-snapshot discovery.
   Assert actual window focus and native input focus identity, not visual styling.
2. First demonstrate ordinary keys reach the installed production bindings.
   Begin native marked text and send Up, Down, Enter, Escape, Tab and Shift-Tab,
   including explicit and implicit repeats and releases, in the same visual
   update, with no executor ticks, draw, sleep or engine drain in between.
   Native composing state must already be true while controller draft composition
   is still false, proving the pre-engine-update window rather than a settled
   no-binding state. Because NSH owns an independent engine thread, hold the
   existing SharedDraft mutex across mark and keys: the composition callback
   cannot complete or publish its replacement keymap until the guard is released.
   No new production hook or engine substitute is needed. Check each gesture in
   an isolated mounted fixture so opposite navigation/scope commands cannot
   cancel and hide a defect.
3. At that boundary and after bounded engine processing: no activation, close or
   committed-query event; selected result, scope, controller query and native
   focus identity unchanged. After processing, shared draft composition must be
   true and native marked text preserved. Repeat the gesture after processing
   and retain the same assertions.
4. Unmark through the real AppView API. Exactly one committed-query event with
   the committed text, no deferred command replay, composition false and focus
   unchanged. Apply actual `PaletteSearch` results, then show ordinary navigation,
   scope cycling and activation work again once per intended action.
5. Independently cover ordinary navigation repeats, nonrepeat Enter/Escape/Tab/
   Shift-Tab, forward/backward scope cycling, exact activated targets and exact
   close counts. Native typing must produce the committed controller query and
   exactly one query event without activating or closing.
6. Preserve existing draft/session/projection/lease regressions. Add a mounted
   controller check that actual requests captured before a draft edit, projection
   replacement or unmount cannot emit activation/close or change selection/scope.
   Such private authorization probes supplement, never replace, public-key tests.
7. Scheduler drains stop at 512 ticks or 2 seconds; each wait stage stops after
   10 seconds. The harness runs sequentially on its one main thread and reports
   exact passed/failed/unrun case counts, including setup/teardown failures.

## Implementation plan

1. Inspect palette controller/actions/results, Surface entity access, official
   mounted host examples and the pinned vendor editing API.
2. Write this acceptance and plan before modifying Rust (this section).
3. Add a feature-gated child fixture with real event subscriptions, bounded input
   discovery, native focus observation and the three behavior cases above. Use
   the existing `nsh-reading-integration` feature. Bounded discovery needs no input
   observation hook. A minimal gated render observation may track the actual
   projection, selected/scope, draft revision and composition/focus signals to
   avoid mistaking an unchanged ProjectionId for completed navigation.
4. Add minimal gated re-exports and a harness-free single-main-thread runner.
5. Run only authorized static source/format checks; do not compile or execute
   Rust. Re-read final files, measure exact SHA-256 values and hand parent the
   manifest stanza, next commands, remaining limits and honest unrun counts.

## Evidence boundary

The fixture uses `AppView::integration_edit_snapshot`, `integration_edit_focus`,
`integration_edit_type`, `integration_edit_mark`, `integration_edit_unmark` and
`integration_edit_key(InputKey)`. The last uses the actual public native key route;
it does not invoke the GPUI Window event closure or OS input method/candidate UI.
Direct/component replay and host propagation remain the vendor regressions'
responsibility; the mounted palette suite does not claim those paths as covered.

The supplied backend repair hashes are expected prerequisites, not passed tests:

| File | Expected SHA-256 |
| --- | --- |
| `vendor/nsh-platform-gpui/src/input.rs` | `b61dd1f487fd5863af811d4bc0130cdf442cb583453ad8c968f7083b92b251a8` |
| `vendor/nsh-platform-gpui/src/events.rs` | `7be330b4044795c3dfdf71b957e5a01420dd7c95f154affd6532b399b6ae6148` |
| `vendor/nsh-platform-gpui/src/text_input/host_tests.rs` | `81a07509bac52becac0f60042afbe14d7c5f92ba985b3af1453e70494efb2d6c` |

## Authored handoff and static evidence

This author has stopped implementation writes. HEAD remains
`0b5ea426abfd8d4358489e916273d89474fb190f` on `feat/907-nsh-full`.
Only the five authorized paths were written. No manifest, lockfile, lib.rs,
other export, vendor, text_input/sidebar, anchors, timestamp or ordinary-control
file was written by this author. No Cargo/rustc invocation, network, Git mutation,
OS-input synthesis or application launch occurred.

The three public, feature-gated entries are:

- `command_palette::marked_palette_keys_leave_controller_and_focus_unchanged`
- `command_palette::ordinary_palette_keys_repeat_only_navigation_and_emit_exact_events`
- `command_palette::retired_palette_requests_cannot_mutate_mounted_controller`

The first case has six independent fixtures, one per gesture. Every fixture first
proves ordinary production Down/Up registration, then holds the existing draft
mutex while the native mark and complete repeat/release gesture run in one visual
update. Composition is immediately true in the native snapshot but false under
the locked controller draft. It then checks settled composition, repeats the
native gesture, commits by unmark and verifies ordinary keys and exactly one task
activation. The other cases cover ordinary repeats/events and actual receiver
rejection after draft replacement, projection replacement and mounted lease
expiry. Private requests supplement the public-key route only.

The only presentation observation is an effect gated by
`nsh-reading-integration`, after production `register_keys`, recording the actual
snapshot/selected/scope/draft revision and actual composing/focused signals.
It does not replace or intercept keys, create nodes, alter commands or fabricate
focus. It proves engine observation, not that a main-thread mount batch was
applied. Real window frames, controller effects and native focus checks remain
separate. `ElementRef::measure()` was inspected and rejected as a main-thread
acknowledgement because its result is delivered on the engine thread. If parent
execution reveals a remaining observation race, fix that test observation layer,
not production behavior or assertions. An actual production failure must be
reported to parent; no fix/bypass is authorized here.

The local cached `gpui-fast 0.1.9` API was inspected: entity reads outside a visual
update use `read_with`; wait predicates receive `&App` through
`TestAppContext::read`. The initial authored TestAppContext read mismatch was
corrected from that signature evidence before compilation. No runtime failure or
production defect has been established by this uncompiled source.

Static commands actually executed successfully:

- Direct toolchain `rustfmt --version`: `rustfmt 1.9.0-stable
  (88d9e12ae1 2026-08-18)`.
- The same direct `rustfmt --edition 2024 --emit stdout --config
  skip_children=true`, fed each of the four final Rust files on stdin: exit 0,
  byte-identical output on the final pass. This is parsing/formatting only.
- `git diff --check -- crates/vega_ui/src/command_palette.rs
  crates/vega_ui/src/command_palette/presentation.rs`: exit 0.
- Python byte checks: no comments or lint suppressions in the four authored Rust
  files; removing only the gated insertions reconstructs the two original hashes:
  command_palette.rs `9e344e439df73682f1aba13da0e1111bf5d55268cb2e23176bed7b18d65de08c`,
  presentation.rs `9968f6567b0f2f58772df3a90851b59fdf60da3eb16333d3aac3b362faa1c9a3`.
- Existing `presentation/tests.rs` remains byte-identical at
  `55598e94e86fc674ff137ed15b8912201ac58c0387589b41a5ed7c17d4e89283`.
  All three vendor prerequisite hashes above were remeasured and match exactly.

Final authored Rust hashes:

| Path | SHA-256 |
| --- | --- |
| `crates/vega_ui/src/command_palette.rs` | `1d758c2cc0b53d14fa68ae441f4539d4cc47c91fd913920e20bdb956a2ff1c42` |
| `crates/vega_ui/src/command_palette/presentation.rs` | `58dc50dedacf5bed1a9b602e1a1feffbee47aa8c520b90675205a0e043b98bd8` |
| `crates/vega_ui/src/command_palette/presentation/mounted_regressions.rs` | `f330c096518eb34b4b26a77e998169f65bc4586bcd653a5d45eaf1ce27db298d` |
| `crates/vega_ui/tests/nsh_palette_integration.rs` | `1b8b972019d97b0805124704e22af4cc5e167e45149bab30944271945783683c` |

## Parent minimal wiring and next commands

Parent must add only this target stanza to the existing vega_ui manifest; the
existing feature already enables GPUI test support and vendor host regressions.
No lib.rs wiring or new dependency is needed:

```toml
[[test]]
name = "nsh_palette_integration"
harness = false
required-features = ["nsh-reading-integration"]
```

Next coordinated command after that wiring (not run by this author):

```sh
cargo test -p vega_ui --test nsh_palette_integration --features nsh-reading-integration --locked --offline -j 2
```

The harness runs all three cases sequentially on its main thread, prints exact
passed/failed/unrun counts and stops on first case failure. Setup/teardown panic
is reported separately and exits nonzero; it is not counted as a case passing.
All waits retain 2-second/512-tick drains and 10-second stage deadlines. Parent
should retain an external process timeout as used for other mounted suites;
these cooperative budgets do not preempt an individual hung native operation.

Required follow-up commands, also unrun here:

```sh
cargo test -p vega_ui --lib command_palette::presentation::tests --locked --offline -j 2
cargo test -p vega_ui --test nsh_native_host marked_keys_never_reach_shortcuts_or_waiting_keys --locked --offline -j 2 -- --exact
cargo test -p vega_ui --test nsh_native_host --locked --offline -j 2
cargo check -p vega -p vega_ui --features vega_ui/nsh-reading-integration --locked --offline -j 2
```

Results at this handoff: **0 passed / 0 failed / 3 unrun** new mounted cases;
existing palette regressions, repaired vendor host regressions, type checking,
Clippy and CI are **NOT RUN**. Actual Cargo/rustc versions are **not obtained**
because invoking them was prohibited. Parent must obtain fresh toolchain/content
hashes and logs before claiming behavior. This is not independent review, final
head acceptance or native OS IME/candidate-window acceptance. Development Rust
tests and gated support still require archival/removal before master.
