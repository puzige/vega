# #907 — explicit NSH keyboard adaptation

Baseline: `b301b64e00690365087362ba21ec45a96f35400f`. Fixed NSH 0.4.0 / Fast host; no dependency upgrade or backend fork.

## Acceptance before implementation

| Scenario | Required result | Evidence |
| --- | --- | --- |
| Button-only settings navigation receives real host focus | Tab/Shift-Tab select enabled controls in displayed order; visible ring follows only actual keyboard focus | Policy regressions, static host integration; native hand test pending |
| Reading-order ends | Leave the composite toward the next/previous external focus stop; do not silently trap focus across sections | Host source inspection; native hand test pending |
| Enter/Space | One non-held activation through the original displayed authority command receiver; disabled/stale/hidden controls reject | Existing authority regressions plus shared selection tests |
| Route/session replacement | Old selection cannot authorize a new presentation; reconcile against current displayed eligible controls | Development tests |
| Pointer click | Original NSH pointer handler remains the only pointer activation path; no native hitbox covering NSH product controls | Static element tree and action path |
| Accessibility | Paint-free active descendant represents selected control under a real focused host ancestor; original NSH product semantics remain | Static proxy construction; native AT pending |
| Inputs, nested popups, scroll/reveal | Not silently covered by button-only helper. Input/IME ownership, modal scope and exact scroll observation require separate explicit adapters | Remaining scope, not claimed passed |

## Chosen implementation and rejected assumption

Pinned `collect_stops` includes Input and focusable Native, not ordinary View. Native component registration is available, but a full-size component inserts BlockMouse and would intercept original NSH pointer/wheel delivery. A zero-size component has unresolved accessibility bounds and ordering issues; neither is accepted as a transparent fix.

First extract a small reusable selection and real-host event policy for **button-only composites**, then adopt it in the settings shell navigation. GPUI stays a paint-free focus/event/AX host; all visible labels/icons/background/rings remain NSH. Selection includes presentation identity and caller-supplied eligible reading order; caller still owns displayed authority validation. No ambient global `When::Always` Enter/Space handlers and no secret/input values enter this protocol. Tab exit uses the native window chain while skipping the same composite subtree. If the window contains no external stop, retain valid boundary selection rather than swallowing a nonexistent transition.

This is not completion of cross-product keyboard parity. Provider forms, Composer, mixed input/control surfaces, nested modals and virtualized/reveal targets remain explicit follow-up adoption and acceptance work within #907.

## Implementation validation

- Initial compile exposed an unsupported element `on_focus_in/out` API. Replaced with Context subscriptions retained by the presentation. Independent review then found tracked FocusHandle does not inherit a div Tab flag, and child AppView remains a reverse-entry stop. Corrected the handle itself to `.tab_stop(!heading)` and focus-in of either host/descendant synchronously initializes first/last selection and transfers to host. Independent incremental static review closed the P1 finding on hash001c59fa: Context subtree-entry subscription does not loop on child-to-host transfer; exit's intermediate focus changes do not dispatch immediately and current-containment check prevents reclaiming external focus. No remaining concrete finding in that delta; no native success is inferred.
- Stable integrated check: `cargo check -p vega_ui --locked --offline -j 2` exit0,6.03s; `cargo clippy --workspace --all-targets --locked --offline -j 2 -- -D warnings` exit0,7.86s; `cargo fmt --all -- --check` and `git diff --check` exit0. Cargo fork manifests report preexisting unused-dependency warnings; Rust strict Clippy succeeded.
- `cargo nextest run --workspace --locked --offline -j 2` found/executed/passed87/87/87, skipped0, run `c40ad357-f27d-45c3-89af-65b36f562866`, build15.13s, execution0.403s. Includes three shared selection/key policy tests and twelve ShellRecovery policy tests. These are not native focus-chain evidence.
- Toolchain: rustc1.98.0 `(88d9e12ae 2026-08-18)`, Cargo1.101.0 `(e611c7a87)` / huacnlee0.102.0-9, Nextest0.9.151. Baseline HEAD remains b301b64e with uncommitted delta. Current source SHA-256: `nsh.rs`28702903c61e8eb856bca125eb8ddb37a3bd5274f17a167fc333a12b65a52c72; `nsh/keyboard.rs`3c88128eda36fc4cad4b375f1292aaf179963de94ebb3aa902ba73df32422e0a; `settings/nsh_shell.rs`001c59fa457a543d5cbec1291b3131a0808b5b2176f271b88b8c5036c3cfb15a. Tests remain development-only and must be archived/removed before final merge.

## Development CI checkpoint

`b301b64e00690365087362ba21ec45a96f35400f` cloud run https://github.com/puzige/vega/actions/runs/42365401433 : Clippy SUCCESS, Nextest SUCCESS, aggregate `check (fmt, clippy, test)` SUCCESS. This is the prior implementation stage, not validation of subsequent keyboard changes. No application/native hand test, merge or release occurred.
