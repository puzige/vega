# Consolidated independent review — conversation production UI

## Conclusion

**No actionable P0–P3 finding identified in the reviewed delta and affected application contracts.** This is a non-author, read-only implementation review, not a restatement of author closure claims, a claim of exhaustive line coverage, or native acceptance. No implementation change is requested by this report. Review stops at this frozen version; optional improvements and static-only unknowns do not open additional work.

## Exact version

- Branch: `feat/907-nsh-full`.
- HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`.
- Requested comparison base: `34555cec9c0c4c4c1f7a528d739752f414e53edf`.
- Integrated mainline content reference: `2a29adf9b2ea78eaf59d591838531b45adc7373e` (#916/#920/#922 integration is working-tree content, not asserted Git ancestry).
- Frozen content manifest: `target/nsh-implementation-review-content.json`.
- Content identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`.
- Manifest file SHA-256: `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c`.
- Frozen tracked patch: `target/nsh-implementation-review-tracked.patch`; SHA-256 `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`.
- Before report creation: all 2137 manifest files matched their individual raw-byte SHA-256 values; the identity recomputed using the manifest's declared algorithm matched; the existing tracked/untracked, nonignored file set, excluding `__pycache__`, matched exactly. Current `git diff --binary HEAD` independently produced the frozen patch digest.
- Post-write verification: all 2137 frozen files still match; content identity, HEAD, branch, manifest SHA-256 and current tracked patch digest are unchanged. No frozen path is missing. In addition to this report, two new out-of-manifest documents appeared during review: `docs/907/review-app-settings-workspace.md` and `docs/907/review-native-security.md`. They were not created or modified by this reviewer and their conclusions are not incorporated here. An initial assertion expecting only this report as an extra path failed on those concurrent additions; the individual frozen-byte and tracked-patch checks did not fail. The final membership check separates these three observed report additions from frozen implementation content.
- Review reports are outside the frozen implementation set. The implementation identity is not regenerated to include them.

## Scope and method

Read the applicable root AGENTS review/SDD/UI rules, execution guide, design guidelines, issue workflow, full migration specification and relevant conversation/reading, footer, ordinary-control, timestamp, selector, scroll and mainline-compatibility contracts. No nested AGENTS was found under the reviewed source paths or `docs`.

Inspected current source and focused differences from the requested base; used the integrated mainline reference for recovery compatibility checks. Followed production entry points and affected state/command boundaries rather than auditing unrelated backend implementation. The separately assigned native reading/editing/scroll implementation is excluded; only its application call sites and the limits of its development observation contract were considered. The fixed-version authorized backend extension is not treated as an unauthorized dependency change.

No Cargo, application runtime, tests, network, Git mutations, source edits or delegation were performed. The only repository write is this new document; the explicit frozen-review restriction takes precedence over ordinary fetch/update workflow.

## Independently checked paths and invariants

Paths below are relative to `crates/vega_ui/src/` unless otherwise stated. These are the substantive checks supporting the conclusion, not a claim that every line in every referenced module was reviewed.

| Area | Source evidence and assessment |
| --- | --- |
| Production paint and dispatch | `conversation_stream/render.rs:654–750` and `render_rows.rs:148–268,381–475`: ordinary production user/assistant messages take the interaction-bearing NSH body path. Located legacy Markdown helper callers: the benchmark entry and a subagent branch rendering a permission entry do not establish a reachable production legacy Markdown body. Transparent layout/focus/event wrappers and measurement-only canvas callbacks were distinguished from visible GPUI content. |
| Markdown and selection/copy | `selection_menu/body.rs:241–449,620–761,1234–1431` and `selection_menu.rs:343–439`: checked retained document construction, source-domain continuity, UTF-16 boundary conversion, fragment/source validation, directed selection installation, exact selection/content generations and current body geometry. Copy-original and copy-selection remain distinct; fork and both compaction directions retain trusted controller dispatch and eligibility checks. |
| Source caret and scroll composition | `selection_menu/body.rs:981–1232`, `selection_menu/mounted.rs:16–137`, `turn_placement.rs:145–165`, `nsh/scroll.rs:71–94`, `disclosure/scroll_group.rs`, and `tool_activity_group.rs:236–315`: source restoration checks content, mount, geometry and scroll intent; selected content interrupts restoration. Nested group rendering retains the original ScrollHandle; outer remainder consumption updates original ListState and interrupts follow through placement state. No alternate list or wrapper-only visible replacement was inferred. |
| Footers and current-frame geometry | `message_footer.rs:115–170,236–281,300–443,774–826`: checked owner/source/route identity, live paint/geometry, active action witnesses and trusted fork dispatch. Owner observation compares semantic presentation before stop/notify, rather than notifying unconditionally on each owner notification. Child prepaint uses dynamically scoped current-frame list bounds, not a reentrant ListState viewport read; action-time checks retain the actual list/visibility fence. |
| Ordinary controls and timestamps | `ordinary_submissions/presentation.rs:365–433,605–654`, its `pixels.rs:165–252`, and `timestamps/presentation.rs`: checked submission/attempt identity, unchanged action emission, diagnostic copy eligibility, active-window/mount/viewport checks, and measurement acceptance. ListState geometry collection is deferred to paint in the ordinary row. Control measurement uses page coordinates; timestamp content is rendered by a retained NSH Surface with versioned width/height feedback. |
| Message anchors | `message_anchors.rs:570–621,629–740` and `message_anchors/pixels.rs:98–223`: checked original message-id navigation and privacy-bounded preview, layout-only host measurement versus NSH visible marker/preview, and retained surface handling. GPUI event/accessibility wrappers were not mistaken for independent product paint. |
| Composer | `composer_actions/native.rs:557–704` and `composer_selectors.rs:122–220,406–527`: activation carries the observed projection, rechecks owner/source, current menu/model options, mount and geometry, and uses original send/stop/model/permission/thinking controllers. Running-send retains the composing guard. Model option indices correspond to the projected original option order; stale state is not replaced with a newly captured authority at activation. |
| Thinking and run timeline | `thinking_body.rs:46–140`, `run_activity.rs:1378–1559`, `newrunactivity/mod.rs:581–637`: checked incremental append versus source replacement, source identity/prefix handling, Final/manual expansion ownership, native body/header entry points and current owner/layout/publication checks. Measurement callbacks that settle anchors do not themselves paint legacy product text. |
| Tool cards and #922 recovery integration | `tool_card/nsh_tool.rs:213–227,430–546`, `conversation_stream/content.rs:2208–2240`, `shell_recovery.rs:141–259`, and `shell_recovery/nsh_view.rs:270–399`: shell control retains operation/call/thread ownership, pending deduplication and trusted service event flow. Recovery display, observation, projection, confirmation and sequence identities are rechecked; dismissal does not itself claim service completion. Compared application recovery changes against the integrated mainline reference, without duplicating backend/service/store review. |

## Existing validation evidence — inspected, not rerun

Read `target/nsh-implementation-closure-execution.json`, relevant stdout summaries, `target/nsh-implementation-closure-final-clippy.stderr` and the fmt stderr. The execution record reports exit 0 and no watchdog timeout for all eleven UI/native/integration entries. Located summaries report UI 358, native host 36, geometry 24, reading 15, scroll 16, icons 3, settings 3, recovery 3, anchors 6 and ordinary controls 4 passing. Palette has an exit-0 receipt; this review does not independently derive its claimed count of 3. Clippy's retained stderr reports completion in 12.73 seconds; fmt stderr is empty. Empty stderr alone is not an independently executed fmt pass.

These are retained author-run artifacts, not new reviewer-run successes or independent proof of their execution-time source identity. Relevant inspected development tests include source-caret selection invalidation, footer feedback convergence and owner revocation, ordinary-control stale-route/unmount cases, and mounted message-anchor lifecycle cases. Passing counts do not by themselves establish NSH paint ownership or business-semantic preservation; those conclusions above are source-based.

## Limits and remaining independent gates

- No runtime reproduction, build, new test execution, screenshot or OS event/clipboard inspection was authorized. Static review cannot establish scheduling behavior, physical pointer/focus operation, IME candidate behavior or visual correctness on actual desktops.
- The read-only current-text development seam observes current native text/layout records; it explicitly does not prove painted pixels, accessibility exposure or identity across snapshots. No manufactured TextLayout result is accepted as native evidence here.
- macOS/Windows/Linux native acceptance, OS accessibility trees and real assistive-technology operation remain unverified. Transparent GPUI accessibility/focus wrappers are permitted by the application architecture but are not proof of OS AX behavior. Native scrolling/gesture and caret APIs are assumed only at their documented application boundary; their implementation is the other reviewer's scope.
- Latest final-head required CI, recoverable development-test archival/removal before master, complete consolidated review across other assigned domains, and user hand testing remain separate delivery obligations. This report neither performs nor waives them and does not claim merge/release readiness.
- No optional improvement is promoted to a blocker, and no out-of-scope backend investigation is requested.

**STOP — review complete for the exact frozen implementation content above.**
