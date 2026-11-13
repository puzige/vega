# Independent incremental security review — single-line paste P2 fix

## Verdict

**No actionable findings in the reviewed four-file delta. The original P2 single-line transactional-paste finding is resolved in these exact current bytes.** This is a non-author static delta review, not a repeat of the migration review, full-migration approval, runtime validation, or native OS acceptance. The historical reviewer’s `review-native-security.md` remains unchanged.

Applied `AGENTS.md` independent-review scope/termination rules and the relevant execution-guide safety, authority, evidence and delivery constraints. The explicit frozen/read-only review authorization overrides the ordinary fetch/update requirement; no fetch or branch update was performed. Implementation was supplied stopped and remained byte-identical at verification points.

## Exact reviewed identities

Worktree: `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`.

- HEAD verified: `0b5ea426abfd8d4358489e916273d89474fb190f`. HEAD alone does not identify the dirty implementation.
- Original frozen whole-manifest identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc` (2,137 recorded path/hash pairs; recomputed, not a claim that all current files still match).
- Original frozen manifest raw SHA256: `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c`.
- Original frozen tracked patch SHA256: `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`.
- Recovery manifest SHA256: `79671cd315b385d58e3f999cded78e94f1d9b8e94919c63370e8f41294dadbad`.
- Complete four-file `frozen-to-current.patch` SHA256: `021362408debfaab7b10e107db4b0a94c98894e6d109a9b1c2d72ad308ecaf05`.
- Recovered PRE-P2 four-file identity: `e775b1648b9d657d9169e69f18d94dde7aadb9bb7b278665ce7bb77d25352e94`.
- Reviewed current four-file identity: `125bd6a51d143d483c69501ec70c317bf551114199b0c58b921d7fcf5144cced`.

Four-file identities use SHA256 of `json.dumps(sorted((relative_POSIX_path, SHA256(raw_bytes)) pairs)).encode()`, with full repository-relative paths. Paths in this table are relative to `crates/vega_ui/src/`:

| File | PRE-P2 SHA256 | Current SHA256 |
| --- | --- | --- |
| `text_input/native_adapter.rs` | `d9144ef7b2e6626b3062e7a28945c06924cde48b38342cf7ea4badb149593155` | `7b4d9d0a7f87b04ff203d6091ac320754f1915e142a55c22e8b566d013add7d9` |
| `text_input/editing_core.rs` | `5ea00d864cb939b7f7868c4c85443a9dd603cf7804c892e8155a520c8969c1ac` | `cb037e051edcaea4eee4634b2e7b74435d31a3263c46b59ecc779a5ceff3c0f0` |
| `text_input/nsh.rs` | `8f71c554bc9643c50e03f8ad7ad4f4a78dc84ff789c3ffcd9ad88625d621dc59` | `fd29c6714f18c272a54c5ecc09f2526e294929a2e053e39b6d1b6b5ba23fad47` |
| `sidebar/threads_block/rename_tests.rs` | `42ada6277b0519fa6d3a54de5a7fca1ac4f79572c6973580897cab1ceb283139` | `5ef9e8df876dfddd8c54b7cb74c8308ed3b37374f24456b3227172bb100d34d1` |

Independently hashed each persisted `pre-p2/` file against its entry in the original frozen manifest and recovery manifest; checked sizes, current snapshots and live source equality; recomputed both four-file identities; regenerated the complete unified diff in memory and obtained byte-for-byte equality with the supplied patch. The recovered files therefore supply exact preimages for this review. As documented in `nsh-paste-baseline-recovery.md`, three preimages are hash-validated inverse reconstructions, not archived original copies; this does not establish additional provenance beyond the supplied frozen evidence.

## Affected invariants checked

1. **Explicit policy and legacy bytes.** `editing_core.rs:198–219` defines normalization on `NativeEditorPolicy`, not on image admission or an inferred multiline setting. SingleLine replaces LF with space and preserves CR: `alpha\nbeta` → `alpha beta`, `alpha\r\nbeta` → `alpha\r beta`, lone CR unchanged. This matches the actual legacy branch at `state.rs:657–665`; preserving CR is intentional scope, not a new flatten-all-breaks policy. Composer retains CRLF → LF, with LF and lone CR unchanged. Session entity identity participates in policy equality; the mounted route supplies it explicitly (`nsh.rs:145–150,1223–1234`).
2. **Consistent authorization and application.** Adapter preparation (`native_adapter.rs:298–345`) retrieves the captured policy and uses it for both expected text/selection and insertion. `pasted_text` (`nsh.rs:1496–1518`) preserves directed UTF-16 range handling and computes the resulting caret from derived insertion bytes. Authority (`nsh.rs:594–710`) requires the queued serial’s policy, validates expected text/selection and exact mutation insertion/range, then applies replacement with that same policy. The non-paste path does not use paste normalization. No layer-only repair or Composer normalization change was found.
3. **Policy/session/focus ABA rejection.** Begin captures policy, policy epoch and focus epoch (`editing_core.rs:399–475`). Preparation and completion require current context equality. Policy changes increment a checked epoch, so switching away and back cannot revive an old paste; same-policy route refresh does not spuriously invalidate it (`635–647`). Every accepted focus notification advances a checked focus epoch (`382–397`), including refocus. After completion, context is bound to the explicit serial. Authoritative `commit` rechecks context under the same state lock that covers the application closure (`523–559`), closing the gap after the earlier policy lookup. Changed context cannot invoke authoritative application; overflow fails closed.
4. **Queue, authority and carrier fences preserved.** Existing owner/mount/focus/composition checks, ordered serial/snapshot/revision validation, pending acknowledgment and failure recovery remain. Each successful paste observation adds one context only after admission to the existing queue bounded at 64 edits; ordered commit removes its context. Retired/failed bridges cannot authorize application. The original clipboard carrier is not normalized or replaced: only derived strings change. Weak allocation identity, unique completion ownership, `Rc::try_unwrap`, original metadata/image/path handling and existing budget reservation remain intact. No new clipboard acquisition, attachment capability, filesystem path or authority bypass is introduced by this delta.
5. **Regressions inspected, not executed.** Added development cases cover both policies with LF/CRLF/lone CR, reversed non-BMP selection, original opaque metadata/carrier retention and release, pending acknowledgment, and policy/session/focus changes before preparation, completion and authority. Actual hook → adapter → controller authority coverage is present in `nsh.rs:2641–2757`. The mounted rename extension (`rename_tests.rs:310–369`) uses native select-all, waits for acknowledgment, pastes each line-ending case, waits for paste acknowledgment, presses Return and checks persistence; it restores the prior title before continuing the existing cases. Existing helper expectations remain unchanged with explicit Composer arguments. No weakened assertion, deadline, scheduler limit or removed regression was found in the diff.

## Validation and limits

Reviewer-executed checks were read-only source inspection, SHA256/JSON identity checks and in-memory diff comparison, all successful. No Cargo, compiler, formatter, test runner, app/OS/AX launch, network, Git mutation, implementation edit or delegation was performed. The sole write is this new report; the original report and implementation are not edited.

The current request supplies author/parent current-cycle results: UI **362/362**, reading **15/15** including native paste acknowledgment/submit persistence, and **14 full artifacts with exit 0**. These are supplied execution evidence, not tests rerun or independently reproduced by this reviewer, and not OS acceptance. The fix note’s earlier unrun status is an author-stage record, not a claim that those later parent results did not occur.

Actual OS clipboard acquisition, native focus/menu/IME delivery and cross-platform native acceptance remain unverified here. This verdict is limited to the four exact current files above and their affected invariants; it does not reapprove unrelated dirty migration content. Archive/removal, development-test cleanup, complete final-head required CI and user hand testing remain separate parent-owned gates. Later source changes need identity reconciliation and any applicable incremental review; this verdict does not claim those future bytes are reviewed.

Evidence is sufficient for this scoped independent verdict. Review ends with no actionable findings; no implementation or additional investigation is opened.
