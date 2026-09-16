# Utility chip states and branch row geometry

User-authorized task, 2026-09-16. Supersedes R49 chip geometry and the recent draft-branch chrome only as listed here. Sources: user-visible screenshots and Vega code; third-party extracted code/assets are not implementation inputs.

## Observation and decisions

Reference screenshot shows a highlighted branch chip while its menu is open and an unfilled Local chip. This supports transient hover/open rather than permanent fill, but a still image cannot separate simultaneous hover and open. Reference pill spans about 56 physical pixels on the supplied 2x-scale image, consistent with the user's chosen 28 logical px target. Sampled lower pill pixels are around (230,231,231)/(231,232,232); upper pixels vary with the popup shadow. These samples do not prove a private overlay algorithm.

## Requirements

R1. Both Composer utility chips: height 28px, horizontal padding 8px, full capsule radius (effective radius14), existing 13px text and16px icon unchanged. Existing three-node GitBranch is already correct; retain it. `!chip_chrome` R19 path remains byte-for-byte equivalent in behavior/geometry (32px trigger).

R2. Three paint states for both: rest transparent; pointer hover filled; open filled even after pointer leaves; closed+pointer-away transparent again. Overlay is paint-only and never dims text/icons or changes hit testing. Use a dedicated shared theme color: light neutral #DBDBDB at alpha0.6, dark white at alpha0.10. These are independently chosen Vega values to approximate screenshot contrast, not claims about third-party internals. Light over actual utility surface #FAF9F9: 0.6*219+0.4*250=231.4, G/B=0.6*219+0.4*249=231; rounds #E7E7E7. Dark over #191919: .1*255+.9*25=48 => #303030. No independent selected state exists on these triggers. Do not alter menu selected-row colors or global hover tokens.

R3. Chip bounding-box gap 28→8px via existing theme token. Internal icon-label gap remains8px. Thus text edge to next icon includes right padding8 + gap8 + left padding8 =24px; do not mistake text gap for chip gap. Utility bar height37 and leading inset14.5 unchanged. New chip center inset=(37-28)/2=4.5px. Theme owns new chip height/padding/color values, update frozen tests/docs for deliberate contract changes.

R4. User screenshot also exposes branch row shrink-to-content. Make branch rows span menu content width minus their existing4px edge gutters on each side, and selection marker occupy trailing column. Do not change MENU_MAX_WIDTH, MENU_RADIUS, menu height cap, menu row32px height, font, selected-row colors, or Git controller. Preserve bounded virtual list and scrolling; do not force a tall blank popup or add placeholder functionality.

R5. Preserve upward anchoring, deferred painting, project popup350px width, capture_any_mouse_down, on_mouse_down_out, search, Esc and real draft branch access. No dependencies, no non-test unwrap/expect; preserve why-comments citing R1–R5.

## Verification and mutation checks

Production painted_quads tests for both chips, light+dark: rest no fill; real simulated pointer hover fill; open then pointer away fill; closed away no fill. Convert quad pixels with scale_factor and inspect as_solid. Assert actual mounted28px bounds,8px padding,8px inter-chip gap, radius14, icon retention, R19 geometry unchanged. Assert branch row width and right-marker location using actual uniform_list, not isolated row.

Four mutation experiments, one at a time, restoring exact working patch before next: M1 force rest filled; M2 remove hover fill; M3 remove open fill; M4 revert geometry (32px height/28px gap and branch row shrink). Each must fail a named production test. If M4 bundles edits, also run each geometry edit independently to avoid one assertion masking another. Record exact changed path, command, failure/assertion, restoration and clean rerun. Do not mutate user state. Shared build lock mandatory.

Run ./scripts/cargo-lock.sh test --workspace; cargo fmt --all -- --check; ./scripts/cargo-lock.sh clippy --workspace --all-targets -- -D warnings. Native verification after packaging, distinguish dark native not-run from automated paint evidence. Report files, counts, mutation failures, dimensions/colors/calculation and residuals in docs/vega-utility-chip-states-delivery.md.
