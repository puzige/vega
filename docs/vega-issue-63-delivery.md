# Issue 63 · Image attachments integration acceptance

## Freeze

- Status: ACCEPTED. Automated gates, native persistence checks and user-confirmed
  successful live vision acceptance are complete; local integration follows.
- Evidence started: 2026-09-18T17:07:43Z / 2026-09-19 01:07:43 +0800.
- Branch: `feat/issue-63-image-attachments`.
- Task contract: `vega-issue-63-image-attachments.md`, R1–R7 and acceptance 1–5.
- Staged code diff SHA-256 (crates, Cargo.toml, Cargo.lock; docs excluded):
  `97512a7c70e39ce99cd99219c46993938b87e6dcc8ec268cea629d12cc2e8547`.
- Environment: Darwin arm64; rustc 1.98.0; cargo 1.98.0; Git 2.55.0.
- Evidence directory: `/private/tmp/vega-issue63-acceptance.0KSLa5`.

## Production-path evidence

Backend and UI ownership, exact focused commands and limitations are recorded in
`vega-issue-63-backend-delivery.md` and `vega-issue-63-ui-delivery.md`.
Those focused passes prove production handlers and persistence/provider wiring
with an owned local HTTP/mock provider boundary, not live vision capability.

The UI suite exercises actual paste, drop, picker and removal handlers, route
retirement, pending import, atomic rejection and durable-ACK ownership. The app
test covers an image-only first submission through provider preflight and lazy
thread materialization. Backend tests record byte-exact request bodies, preserve
images across tool rounds and history rehydration, reject unsupported/oversized
inputs, and exercise attachment transaction rollback and migration compatibility.

## Full gate results

`scripts/cargo-lock.sh test --workspace -- --test-threads=1`: PASS, **1304
passed / 0 failed / 9 ignored**, including doc tests. Raw log:
`workspace-tests-final.log`, SHA-256
`f9fd1ceb60863a749be243b36bda90e8cf888fbcbc66b5ca0d8b5f1f4cd8fa2b`.
Application tests: 150 passed (48.13s); conversation: 319 passed / 3 ignored
(289.82s). The log, not those selected crate subtotals, is the complete evidence.

Additional gates (all through `scripts/cargo-lock.sh`):

| Arguments | Result | Raw log SHA-256 |
|---|---|---|
| `fmt --all -- --check` | PASS | `a0f992ff27524d834d6092d7e3903a90fe68cc80151de059c336eb9158ea7a98` |
| `clippy --workspace --all-targets -- -D warnings` | PASS, 8.29s | `c6c6132be2afde93917c12d0890fa225acf8abee5d9b9c876e90d9227d137e0e` |
| `test --workspace -- --ignored --test-threads=1` | PASS, 9 passed / 0 failed / 0 ignored | `b5b1f76d38057e52b462ade9746469cd3dea7d3d113087b3190027c1396dcf49` |
| `build --workspace --all-targets` | PASS, 6.41s | `116e64eab96d0d89d238da5423aa37704ab88c5c946b91713e126f9d075a90e8` |

The corresponding logs are `fmt-final.log`, `clippy-final.log`,
`ignored-final.log`, and `build-final.log` in the evidence directory. Combined
active and separately run load-sensitive tests: **1313 passed**, no skipped
tests left in that inventory. Existing upstream `block 0.1.6` future
incompatibility warning remains; strict lint emitted no application warnings.
`scripts/cargo-lock.sh run -p xtask -- package`: PASS; release rebuild 27.7s,
ad-hoc signature verified and bundle plist valid. `package-final.log` SHA-256:
`135d0ca9d492c4bf0ce839374698ee3d34aa253d3665e1b1604a7c35386ca95a`.
`scripts/cargo-lock.sh tree -p vega_runtime --edges normal`: PASS, no GPUI or
vega_ui dependency in `runtime-tree.log`.

Installed executable SHA-256:
`8d8fcd3ffa41c0bbd0800cfaa4f32287fa97de7db0ab169030ecbda236bef59f`,
matching the packaged binary. Before updating, observed an idle empty composer,
quit the app and verified its process had exited. Previous app and consistent
SQLite backup retained in `/private/tmp/vega-issue63-backup.UjG2pr`.
Read-only post-launch verification: schema version 7, zero attachment rows
before submission. No user configuration or credentials were changed.

## Native UI evidence

Via native computer-use actions on the installed bundle, not a test-only UI:

- Opened a new draft, explicitly removed project association, and selected an
  available configured model without changing existing conversations.
- `+` -> `添加图片` opened the native file picker. Selecting the owned 320x180
  PNG rendered two red squares and one blue circle in the composer thumbnail.
- Empty text plus a valid image enabled Send. Clicking the thumbnail removal
  control removed the preview and restored the disabled empty-message button.
- Reimported that same PNG and entered a question requesting colors, shapes and
  counts without including the expected answer. Text and image coexist visibly.
- Native paste reported a clipboard-consumption timeout, but the subsequent
  screenshot confirmed the text was inserted once; did not blindly retry.

Screenshots are retained in the task's native tool trace. After the user explicitly
approved the test-image upload, sent the owned PNG through the configured model.
The original request returned HTTP 502. A single same-model follow-up using that
image history also returned HTTP 502. No successful visual answer was obtained.

To distinguish image-specific failure from general model-service failure, created
a separate image-free conversation on the identical provider/model and submitted
only a request to reply OK without tools. That control also returned HTTP 502.
This shows failure is not exclusive to image input; it does not establish whether
the underlying cause is model routing, upstream availability, or another request
compatibility issue. No credentials, endpoints or permission settings were changed.

Native route/restart recovery: PASS. Navigating away and reopening preserved the
submitted PNG. Quit the idle app, verified process exit, relaunched the installed
bundle and reopened the image conversation: thumbnail and both failed-turn records
were still visible. Read-only store check found one attachment of 2045 bytes,
matching the owned fixture's encoded size, with no duplicate on follow-up.
User subsequently supplied a successful native screenshot and explicitly stated
"验收通过". The screenshot shows an attached welcome-screen image and a reply
identifying its heading, subtitle and empty-state layout, with the visible model
selector `deepseek-v4.1-flash`. This is **user-performed live acceptance**, not a
claim that the integrator's earlier model requests succeeded. Together with the
integrator's native route/restart evidence, acceptance item 5 is satisfied.
The screenshot remains in the user conversation, not uploaded to public GitHub.

## Failed attempts retained

1. `workspace-tests.log`: frozen ignored-test inventory rejected a temporary
   native fixture generator. Removed that generator from final source rather
   than weakening the ignored-test guard; retained the owned generated PNG.
2. `workspace-tests-rerun.log`: an existing exact table inventory omitted the
   authorized migration 0007 table. Audited corresponding exact inventories,
   version and count checks; updated them to 11 tables / version 7, retaining
   equality and all historical starting-version fixtures. Focused original
   failure, all 99 store tests and both Todo/S7 integration tests subsequently
   passed. Full serial rerun remains the authoritative whole-workspace gate.
3. `clippy.log`: introduced nested clipboard guard triggered `collapsible_if`.
   Replaced with a let-chain, with no lint suppression. Slice strict workspace
   lint passed afterward; the integration final lint also passed as recorded
   above.

## Residuals and outstanding acceptance

- LIMIT: decoded image output is bounded to 64 MiB; this is not a claim that
  JPEG/WebP internal codec workspace or total process memory is capped at 64 MiB.
- LIMIT: PNG/JPEG/WebP only, including explicit rejection of TIFF-only clipboard
  data and animated images. GPUI may expose text instead of an additional image
  representation; Vega cannot import a representation absent from its event.
- HISTORICAL FAILURE: two image-bearing attempts and an independent pure-text
  control returned HTTP 502 from the originally selected model. Preserved as
  provider-path evidence; user-performed live acceptance later passed on the
  model visible in the supplied screenshot. No general model-availability claim.
- No credential/configuration edits, public upload of user content, remote push,
  or destructive cleanup are part of this acceptance.
