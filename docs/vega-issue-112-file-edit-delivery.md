# Issue #112 — file editing delivery

## Freeze and evidence

Contract: [file editing parity](vega-issue-112-file-edit-parity.md). Implementation branch: `feat/112-claude-file-edit`; baseline `a756559`. Final source identity, toolchain, UTC/local timestamps, full command outputs, durations and SHA256 values are recorded in the external `issue-112` evidence manifest. This report is written before final verification; only the manifest can establish final gate/native results.

## Acceptance matrix

| ID | Requirement | Evidence class and entry | Status at report freeze |
|---|---|---|---|
| A | Canonical strict Read/Edit/Write schema with descriptive file_path fields | strict wire/schema regression; real provider/native acceptance in external manifest | automated implementation ready; live acceptance pending |
| B | Absolute project/sibling Read then Edit/Write | real controller and temporary filesystem tests | targeted tools external-edit regression passed |
| C | External permission prompts, deny, Full, Plan/read-only | production controller tests with real filesystem and MockProvider only | final integrated gate pending |
| D | Shared conversation read state, isolation, stale-after-approval and changed target identity | tools/controller filesystem tests | targeted tools regressions passed; integrated gate pending |
| E | Unique/nonoverlapping duplicate matches, replace_all identity/count, no-op, empty creation/deletion | tools filesystem tests | targeted tools regressions passed |
| F | Quote fallback, exact priority, indentation rejection, CRLF and BOM/UTF16LE roundtrip | tools filesystem and encoded-write restoration tests | targeted tools regressions passed; final gate pending |
| G | Resolved symlinks, pinned approval target, Git/hardlink/checkpoint protection, new parents, >1 GiB | real filesystem security regressions | final gate pending |
| H | Legacy/new history projection, encoded write byte count and external preimage mapping | store/controller codec and filesystem tests | final gate pending |

## Implementation and parity

- Model-visible names are `Read`, `Edit`, `Write`; schemas use `file_path`, exact text replacement fields and optional/default-false `replace_all`. Lowercase tool names and `path` remain an explicit compatibility boundary for restored calls, with identical permission and freshness checks. Both path keys in one mutation are rejected.
- Shared read versions belong to one conversation, keyed by canonical file identity, and retain SHA256 digests rather than full file contents. Existing files require a successful Read before preparation and again before execution. Successful mutations refresh the read version. Same content with a changed timestamp remains valid.
- Absolute external targets and relative paths are resolved consistently for read, permissions, execution and audit. Symlink paths resolve to the actual target; approved mutations pin that identity. Search/shell boundaries are unchanged. Existing hardlink, nonregular-file, Git-control and checkpoint-mutation restrictions remain.
- Exact substring matching takes priority; fallback normalizes only four straight/curly quote characters and maps the match back to real text. Replacement preserves curly quote style. Matching is nonoverlapping, duplicate matches require replace_all, and replacement count reports actual modifications. No whitespace/indentation fuzzy matching is introduced.
- Empty old_string creates a missing file or fills an existing whitespace-only file; an existing empty file still requires Read. Empty new_string follows the reference's trailing-newline deletion rule. Identical old/new or unchanged final output fails.
- Edit normalizes CRLF for matching and restores detected newline style, avoiding CRCRLF from supplied CRLF. Write honors explicit supplied line endings. UTF-8 and BOM-marked UTF-16LE decode losslessly; malformed encodings fail. Regular-file descriptor checks and a 1 GiB limit prevent FIFO blocking and unbounded file loads.
- External preimages are stored under a hashed internal artifact key with a `target.json` canonical target mapping. Absolute user paths are never joined onto the checkpoint root. Historical relative preimages remain readable. Atomic writes, permission denial, post-checkpoint identity/content revalidation and content-free audit remain.
- Write audits optionally bind `expected_written_bytes` when encoding/BOM changes disk byte count relative to UTF-8 arguments. Legacy byte counts remain valid; history/UI restore validates against the encoding-aware value. replace_all changes edit fingerprint identity.
- Errors contain stable codes and concrete corrective instructions without old/new file contents. Legacy error strings remain accepted for restored history.

## Deliberate differences and limits

- This is an independent Rust behavioral implementation, not copied or vendored Claude Code source.
- Existing UTF-8/UTF-16LE BOM is preserved by full Write. The inspected reference can drop it when replacement text omits it; retaining encoding is an explicitly approved data-preservation correction.
- Full file digest comparison rejects changed bytes even if timestamps do not advance, and permits unchanged bytes regardless of timestamp. Reference timestamp and partial-read behavior differs; Vega's stronger content check is deliberate.
- Native Claude integrations (LSP, VSCode, team memory, skills, notebook-specific editing and analytics) are not Vega file-tool behavior and are not ported.
- Ordinary Read of an external checkpoint file follows normal external-read permission rules; checkpoint protection means preventing mutation and storage escape, not a new read ban.
- Project Git artifact/diff restore remains project-scoped; external files receive tool results and checkpoint protection, not project-relative Git artifact actions.
- User-space filesystem revalidation retains the pre-existing residual race against another process changing paths immediately after the final check. In-process mutations serialize. This is not a claim of kernel-enforced global filesystem transactions.
- Controller `MockProvider` tests prove production permission/store/filesystem wiring; they do not substitute for real model/native UI acceptance. Those results must be read from the external manifest before delivery is called complete.

## Verification commands

Initial RED: `scripts/cargo-lock.sh --wait test -p vega_tools issue112_absolute_external_read_edit_and_replace_all` rejected an owned sibling absolute Read with PathEscape. Original failure is preserved in `initial-tools-regression.log`.

Development filesystem suite: `scripts/cargo-lock.sh --wait test -p vega_tools` (intermediate failures from superseded relative-only assertions and added prerequisites retained, never hidden by retries). Final gate selects all affected packages and transitive consumers via:

```sh
python3 scripts/verify.py --plan
python3 scripts/verify.py
```

No dependencies, production user database/configuration or application installation are changed by this implementation task. Native acceptance, packaging, integration and Issue/Project closure remain the main agent's delivery steps.

LIMIT: Dangling symlinks are rejected until their target exists; existing symlinks resolve to their actual target. This fail-closed boundary avoids authorizing an unresolved identity and is intentionally retained.
