# Consolidated independent review — native backend and security

## Result

**One actionable finding: P2 — transactional paste loses single-line normalization.** No additional actionable safety/correctness defect was established in the scoped static review. This is not approval of the full migration or native OS acceptance.

### P2 — Flatten line breaks before authorizing paste into single-line editors

**Primary location:** `crates/vega_ui/src/text_input/native_adapter.rs:319–326` (especially line 324).

The adapter always computes the insertion with `payload.text().replace("\r\n", "\n")`, without consulting the mounted field's single-line setting. The shared expected-result helper makes the same assumption (`crates/vega_ui/src/text_input/nsh.rs:1485–1506`). The transactional backend then inserts the supplied string directly into its model (`vendor/nsh-platform-gpui/src/text_input.rs:2855–2876`); it does not run the ordinary, non-hook paste normalization. Authoritative validation repeats that same CRLF-only transformation (`nsh.rs:624–636,690–697`), so this is an accepted mutation, not an authority mismatch that would safely reject the paste.

**Concrete scenario:** open task-title rename, select the current title, paste plain text `alpha\nbeta`, wait for the ordinary native receipt, then press Return. Rename mounts this transactional editor (`crates/vega_ui/src/sidebar/threads_block/rename.rs:136` via `TextInput::render_nsh_single_line`, `nsh.rs:803–809`). Its accepted controller text contains an embedded LF. `commit_rename` reads that text and passes it to `resolve_rename` (`threads_block.rs:519–525`); the resolver only trims the ends (`sidebar/row_helpers.rs:134–139`), so the update is given `alpha\nbeta` instead of `alpha beta`. Other migrated ordinary single-line inputs share the paste adapter.

**Regression evidence:** the existing editor explicitly flattens LF for non-multiline ordinary paste (`crates/vega_ui/src/text_input/state.rs:657–665`); comparison against base `34555cec9c0c4c4c1f7a528d739752f414e53edf` shows that paste implementation is unchanged. Setting `.multiline(false)` on the new native control (`nsh.rs:1953`) is not an insertion sanitizer. `TextInput::apply_edit` retains replacement bytes (`text_input/bindings.rs:120–126`).

**Required correction:** carry the actual field's paste-normalization policy through admission, expected-result/mutation validation and authoritative application, preserving legacy single-line behavior while keeping Composer newline behavior and the original immutable clipboard item. Do not normalize only one layer, which would turn valid paste into a failed/recovery transaction. Relevant regression coverage should exercise the actual hook → adapter → authority path for LF/CRLF text in a single-line field and unchanged multiline paste. No correction or test was performed by this reviewer.

## Exact reviewed identity

- Worktree: `vega-907-nsh-full`; branch verified read-only as `feat/907-nsh-full`.
- HEAD verified: `0b5ea426abfd8d4358489e916273d89474fb190f`. HEAD alone does **not** identify these dirty migration bytes.
- Migration comparison base: `34555cec9c0c4c4c1f7a528d739752f414e53edf`.
- Manifest-recorded integrated mainline: `2a29adf9b2ea78eaf59d591838531b45adc7373e`; this is not a claim of a new commit or ancestry merge.
- Stopped-content manifest: `target/nsh-implementation-review-content.json`, **2,137** path/hash pairs, excluding the designated pycache content. Recomputed identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`.
- Tracked patch: `target/nsh-implementation-review-tracked.patch`; SHA-256 `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`.
- Every manifest-listed file matched its recorded SHA-256 at review entry and immediately before report creation. The report is a new review artifact, outside that stopped-content manifest. The delivery message records the separate post-write hash verification.
- Backend inventory document: `docs/907/nsh-backend-current-manifest.json`, SHA-256 `73318b7f0de0621ea9537f6704fdbec7a89fc75940c5073ed1ed3b9681cc64fc`.

## Baseline and scope

Read applicable root `AGENTS.md`, execution-guide and issue-workflow review/safety requirements, `docs/907/nsh-full.md`, authorized backend patches, native editing and reading contracts, single-line policy, original-payload paste fidelity and native HTML acquisition boundary. Historical author investigations were not substituted for this independent review.

Compared the actual vendored files to the original-file maps and inspected affected upstream diff hunks and current call paths. Independently hashed/read the local published crate archives without extraction or mutation: core archive SHA-256 `171f17b402bedb0cead975d3abcefbf0e7963583fbdbf1dbcb03ef73ba11191d` and platform archive SHA-256 `1e3828245c26e734567b3e6058398c0e4d5809063636460f990a0eaf8a0f74c3`; archive file maps exactly matched the documented 180 core and 45 platform originals. This establishes local provenance consistency, not network authenticity.

Observed delta: core changes only `src/selection.rs`; platform has the documented fourteen changed originals and sixteen added capability/development files. Versions remain NSH 0.4.0, Kit 0.7.3 and Fast 0.1.9; no upgrade is proposed or authorized here.

Focused review covered:

- Native input identity/mount/focus/registration checks, secure-field exclusion, serial and explicit mutation provenance, initial receipt, ordered adapter delivery, pending acknowledgment rejection, single-claim command tokens and reload retirement.
- Native composition exclusion before direct/held/host routing; candidate/native key handling and restricted existing non-editing shortcuts; explicit single-line submit/cancel admission. No fabricated business tokens or replacement authority were assumed.
- Original Native/Synthetic clipboard carrier, Weak allocation seal and unique completion ownership, bounded reservations, original metadata/image/path preservation, no normalized duplicate native image buffers, rejection/cancellation and original `ImagePaste` handoff. Authorized spare-capacity tightening may transiently reallocate, as the contract explicitly permits; no single-copy peak-memory claim is made.
- Reading snapshot/currentness and exact endpoint installation, copy/context/primary interception, suppression of fallback under an installed handler, and application source-selection validation before clipboard writes.
- Conditional source-caret scroll actuator: current geometry/surface/ancestor validation, finite/busy rejection and synchronous application; inspected caller revalidation without broad UI/follow-policy review.
- Nested backend wheel consumption/remainder and retained route ownership, including weak surface/generation checks, lifecycle retirement and original delta/phase representation. Broad application scroll-composition review remains assigned elsewhere.
- Filesystem boundary: native original external paths flow through existing `ImagePaste` → `paste_images` → `vega_conversation::attachments::import_images`. The existing importer retains `O_NOFOLLOW | O_NONBLOCK`, descriptor regular-file checks, bounded reads and image validation. No alternate bridge filesystem read/write path was established. Platform completeness of that existing importer was not expanded into a separate review.

## Validation and limitations

This was source review plus read-only hashing/comparison, **not execution validation**. No Cargo, compiler, tests, application/runtime launch, network request, Git mutation, source write or child delegation was performed. The sole repository write is this newly created report; the user's frozen-review instruction takes precedence over the ordinary fetch/update rule.

Read existing closure evidence, including logs reporting UI 358, native host 36, geometry/opacity 24, reading/editing 15 and nested scroll 16 passing, and the final Clippy log ending successfully in 12.73 seconds. These are implementation-development records, not checks run by this reviewer and not proof against the finding above. No assertion was weakened and no missing runtime check is described as passed.

Actual OS IME candidate delivery, native focus/paste/menu behavior, shaped visual/caret behavior and macOS/Windows/Linux native acceptance remain unverified here. The documented absence of OS HTML acquisition is a pre-existing pinned-platform limitation, not reported as a migration regression and not authorization to expand acquisition. Required final-head CI, development-test cleanup and user acceptance remain separate parent-owned gates. Application UI/scroll compositions outside the backend boundary were intentionally not duplicated.

Review ends with the finding above and these limitations; no optional investigation or implementation is opened.
