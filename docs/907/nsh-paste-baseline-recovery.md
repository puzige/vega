# PRE-P2 single-line paste baseline recovery

## Result and scope

Recovered the exact four PRE-P2 files required for the interrupted native-security delta review. Each recovered raw-byte SHA256 equals the original value in the frozen manifest and the [paste correction record](nsh-review-single-line-paste-fix.md). All four differ from their current READY versions; no current file was accepted as a baseline.

This is evidence recovery, **not independent review**, implementation, runtime validation, or acceptance. Only this new document and new artifacts under `target/nsh-paste-review-recovery/` were written. Implementation files and existing task artifacts were not modified. No Cargo/build/test, network, fetch/rebase, or other Git mutation ran. The explicit recovery-only authorization takes precedence over the normal fetch/update workflow.

## Exact identities

- HEAD: `0b5ea426abfd8d4358489e916273d89474fb190f`.
- Frozen whole-manifest content identity: `46c7fbd372131b277956f6818af7def7ba8ce1b3912ce8fca377267fea78a3dc`; recomputed from its sorted path/hash pairs.
- Frozen manifest raw JSON SHA256: `9b5d3ccf193668c12833165b216c842d65d8848bf056a84a7d02b6b32c96d71c`. This is not the manifest's content identity.
- Frozen tracked patch SHA256: `6833922e3f600349fe53838137f93ca893de87c2f80a707dfd75c9a0c97eb5ad`.
- Recovered four-file identity: `e775b1648b9d657d9169e69f18d94dde7aadb9bb7b278665ce7bb77d25352e94`.
- Current four-file identity: `125bd6a51d143d483c69501ec70c317bf551114199b0c58b921d7fcf5144cced`.

Four-file identities use the frozen manifest's algorithm: SHA256 of `json.dumps(sorted((relative_POSIX_path, SHA256(raw_bytes)) pairs)).encode()`. They cover only these four repository-relative paths, not the whole dirty implementation.

Paths below are relative to `crates/vega_ui/src/`.

| File | PRE-P2 SHA256 | Current READY SHA256 | Bytes PRE-P2 → current |
| --- | --- | --- | --- |
| `text_input/native_adapter.rs` | `d9144ef7b2e6626b3062e7a28945c06924cde48b38342cf7ea4badb149593155` | `7b4d9d0a7f87b04ff203d6091ac320754f1915e142a55c22e8b566d013add7d9` | 63773 → 71156 |
| `text_input/editing_core.rs` | `5ea00d864cb939b7f7868c4c85443a9dd603cf7804c892e8155a520c8969c1ac` | `cb037e051edcaea4eee4634b2e7b74435d31a3263c46b59ecc779a5ceff3c0f0` | 26366 → 29017 |
| `text_input/nsh.rs` | `8f71c554bc9643c50e03f8ad7ad4f4a78dc84ff789c3ffcd9ad88625d621dc59` | `fd29c6714f18c272a54c5ecc09f2526e294929a2e053e39b6d1b6b5ba23fad47` | 111848 → 120139 |
| `sidebar/threads_block/rename_tests.rs` | `42ada6277b0519fa6d3a54de5a7fca1ac4f79572c6973580897cab1ceb283139` | `5ef9e8df876dfddd8c54b7cb74c8308ed3b37374f24456b3227172bb100d34d1` | 12614 → 14868 |

## Recovery method

Bounded inspection covered worktree rules, relevant execution/delivery guides, task documents, named review artifacts, and the affected source. No broad home-directory or secrets search was used.

1. `nsh.rs`: the frozen tracked patch contains this tracked file. Apply that section to the exact HEAD blob **in memory**, checking every hunk's context and old/new line counts. The resulting bytes match its frozen SHA256. HEAD alone is not the baseline.
2. `rename_tests.rs`: reverse only the added mounted-paste line-ending loop and title-restoration block in memory. The resulting bytes match its frozen SHA256.
3. `native_adapter.rs`: reverse the policy argument and derived insertion, the two added policy regressions and their test import, and restore the original single-line call formatting in memory. The resulting bytes match its frozen SHA256.
4. `editing_core.rs`: reverse the normalization/context/epoch additions, paste-context queue and guards, and restore the original chained route setter in memory. The resulting bytes match its frozen SHA256.

The three formerly untracked files are **hash-validated inverse reconstructions**, not recovered archive copies. Minimal inversion is only a candidate-generation method: recorded original SHA256 equality is the acceptance criterion. Every candidate was validated before original bytes were persisted.

Initial adapter/core candidates failed the recorded digests and were discarded in memory. Correcting the omitted test import/call formatting and chained setter produced exact matches. The first in-memory tracked-patch experiment also failed because the extraction dropped the final context line's newline at the next `diff --git` boundary; the section-boundary reader was corrected, not the source or expected digest. The final strict parser checks hunk contexts/counts and preserves that newline. No failed candidate was published as a baseline.

## Recovery package

All paths in this section are relative to `target/nsh-paste-review-recovery/`:

- `pre-p2/<repository-relative path>`: four exact recovered original files.
- `current/<repository-relative path>`: four current READY byte snapshots, distinct from the baseline.
- `frozen-to-current.patch`: complete unified PRE-P2 → current delta for **all four files**, including development regressions and formerly untracked files; 30614 bytes, SHA256 `021362408debfaab7b10e107db4b0a94c98894e6d109a9b1c2d72ad308ecaf05`.
- `recovery-manifest.json`: exact paths, sizes, methods, input/output identities and limits; SHA256 `79671cd315b385d58e3f999cded78e94f1d9b8e94919c63370e8f41294dadbad`.
- `recover.py`: guarded reconstruction and strict in-memory unified-patch verifier; SHA256 `cf638fd9b2c69b29461ce029971e81637fe76d4f00f801ab68a4de47e5f9f005`. The successful command was `python3 target/nsh-paste-review-recovery/recover.py` (exit 0). It refuses to overwrite an existing package; **do not rerun it against this populated destination**.

The patch uses `a/` and `b/` repository-relative paths. Review it directly or apply it only to a separate copy of the recovered baseline, never to the shared implementation tree. It is a four-file delta, not a replacement for the whole frozen implementation patch.

## Verification and limits

The successful recovery command verified all four frozen/current hashes before persistence, generated the complete delta, strictly applied each file's delta to its recovered baseline in memory and obtained byte-for-byte equality with the current snapshot, read back every generated artifact and checked byte equality, and checked that all four implementation files and existing recovery inputs stayed unchanged during recovery. No byte was written to a source path.

Final read-only verification independently checks persisted hashes, recomputes both four-file identities, checks patch old/new projections against the saved snapshots, compares current source bytes with the saved current snapshots, and verifies the recovery document/package identities. These are static byte-integrity checks, not behavior tests.

No missing preimage remains among the requested four files. This package authenticates them against the supplied frozen manifest; it does not establish additional historical provenance beyond that evidence. Unrelated parent-owned files/gates may change independently. Recovery does not approve the P2 fix, weaken assertions, replace the original non-implementing reviewer, or claim compilation, CI, native/OS/AX testing, or security acceptance. Parent retains gates and the original independent reviewer retains delta review. Recovery writer stops after final read-only verification.
