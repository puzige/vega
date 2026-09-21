# Issue #112 — Claude Code file editing behavior parity

## Authority and scope
2026-09-22 user decision: align native file editing with the supplied Claude Code 2.1.88 behavior, including absolute paths and editing files outside the active project directory. This explicitly supersedes prior relative-only/project-only read/write/edit restrictions for these tools. Existing permission modes, denial rules, checkpoints and race protection remain authoritative. Behavioral reimplementation in Rust; do not vendor the reference distribution or copy its implementation into the public repository.

Reference behavior: Edit(file_path, old_string, new_string, replace_all=false), Write(file_path, content), Read(file_path, offset?, limit?). Exact tool-visible names and parameter descriptions should align with the reference for file tools; legacy stored lowercase tool calls/path audit records must continue to restore correctly. Do not silently accept arbitrary misspellings as aliases. Existing callable compatibility may be retained only as an explicit migration boundary, not competing model-visible schemas.

## Contract
1. Edit exposes strict documented fields file_path, old_string, new_string, replace_all. Logical optional replace_all defaults to false; Chat Completions strict wire can encode null with the same semantics. Write and Read use file_path consistently. Tool descriptions explain read-before-edit, indentation and line-number prefixes, uniqueness and replace_all. Invalid input returns actionable expected shape without echoing file contents or secrets.
2. Exact string replacement; exact match first, then limited straight/curly quote normalization, mapping back to the real source substring. Preserve original quote style as reference does. No broad whitespace/indentation/NFKC fuzzy match. Reject identical old/new content. Default rejects multiple occurrences with guidance; replace_all replaces every occurrence and reports actual count.
3. For existing files (including empty or whitespace-only files), editing requires a successful prior read in this conversation; track canonical identity and original content/version across per-call Tools instances. Reject stale content with a re-read instruction; timestamp-only changes with unchanged full content must not falsely reject. Revalidate after approval immediately before mutation. Read tracking is conversation-isolated. Reference special cases: empty old_string may create a missing file or populate empty/whitespace-only content, but cannot overwrite nonempty existing content. Write existing-file read-before-write and stale checks follow reference behavior. New-file Write needs no preread. Preserve encoding/newline handling supported by reference where applicable; explicitly report unsupported encodings, never corrupt bytes.
4. Absolute file paths, including outside project root, are supported by Read/Edit/Write. Normalize relative paths against active project root for user/backward compatibility; advertise absolute paths as canonical schema. Canonical identity must be shared by read, permission, execution and history. Existing parent paths and new parents follow reference creation semantics. External writes in approval modes go through actual permission UI; full access permits them; plan/read-only rejects writes. No blanket outside-project rejection before permission can be considered. External Read also requires explicit approval in normal/Confirm/Auto/ReadOnly modes (including Ask/Plan), while Full access allows it; this does not grant mutation capability in Ask/Plan.
5. Retain protection for Git control data, checkpoint storage, nonregular files, races, permission denial and audit redaction. Resolve symlinks to actual target for authorization consistently; never authorize one pathname and write another. External target checkpoints must use safe internal artifact keys and retain real target identity, never join an absolute path onto checkpoint root or allow escape. Historical relative checkpoints remain readable.
6. Existing restored conversations and tool cards must accept new canonical schemas/names and old audit projections. Do not leak old/new contents into persisted audit/error cards. Approval cards must identify actual external target clearly. External successful edits remain visible in tool cards with confined checkpoints retaining their real target identity; they are not repository Git artifacts and do not offer repository diff/restore UI. In-project absolute paths retain normal repository artifact behavior. No UI cosmetics unrelated to these behaviors.

## Acceptance matrix (before implementation)
| ID | Scenario | Expected | Evidence |
|---|---|---|---|
| A | Real model produces canonical Edit/Read/Write arguments | valid fields, no missing_old_string retry loop | live provider/native UI + strict wire test |
| B | Absolute path inside project; absolute sibling path outside project | read then edit succeeds under authorized mode; final content exact | real controller + owned temp dirs + native UI |
| C | Outside target in ask/full/plan modes; denial | proper approval, full success, plan/denial zero mutation | production permission lifecycle |
| D | unread file; changed after read/approval; concurrent conversations | reject with re-read guidance; no stale overwrite or snapshot leak | production controller/filesystem |
| E | unique/duplicate/replace_all/no-op/empty old/new | Claude-compatible behavior and exact replacement counts | filesystem regression |
| F | curly quote fallback, indentation mismatch, CRLF/BOM/encoding | controlled match; no incidental context rewrite or encoding corruption | filesystem regression |
| G | path aliases, symlinks, Git/checkpoint paths, missing parents, special files | consistent identity and permission; protected targets unchanged | security regression |
| H | old/new history restore and external checkpoint | safe audit/UI restore and confined recoverable preimage | store/controller integration |

## Plan and ownership
Main agent owns spec, review, integration, issue/project updates and native acceptance coordination. Dedicated implementation agent owns code/tests and delivery report, first inspecting reference source-map behavior and existing specifications. Update superseded spec references explicitly before implementing changed contracts; list any unresolved reference mismatch rather than silently calling partial work 1:1. No new dependencies without main-agent decision.

First establish failing regression for relative-only input/schema and external editing. Implement shared path/read-state/permission foundation, mutation semantics, wire/history/UI compatibility, then run meaningful controller end-to-end checks. Use python3 scripts/verify.py --plan then unified verification with worktree-isolated Cargo scheduler. Save complete evidence outside worktree; do not install or restart user app without coordinating with main agent. Final native acceptance uses owned disposable project and sibling files, never real private user files. No default workspace-wide tests; preserve initial failures.

## Reference details confirmed before implementation
- Reference validateInput permits empty old_string on whitespace-only content early, but its actual call checks lastRead for every fileExists target. All existing files (including empty ones) therefore require prior Read at execution; successful mutation refreshes that conversation's read state.
- Edit normalizes CRLF for matching and restores the majority newline style in the first 4096 UTF-16 code units (ties use LF); Write honors explicit supplied line endings. UTF-8 and BOM-marked UTF-16LE are supported; malformed encodings fail without mutation.
- Empty replacement consumes a trailing newline when old_string omits it and that sequence exists, matching reference deletion semantics. Matching counts are nonoverlapping. Quote fallback selects the actual source substring, then preserves curly quote typography in replacement.
- Audit names remain legacy lowercase and existing in-project paths retain normalized relative display for restoration; external paths use canonical absolute identity. Canonical file_path input and the old path input are mutually exclusive migration shapes. replace_all participates in edit identity; omitted/null equals false.
- External preimages use files/external-<path SHA256> and a separate target.json mapping (version external_preimage_v1, canonical path and artifact); existing relative preimages remain unchanged. User target paths are never directly joined onto checkpoint root.
- Native host-specific integrations (Claude team-memory, LSP, notebook tool, VSCode and skill discovery) are outside Vega file-edit parity scope.

### Approved data-preservation deviation
Reference full Write can omit an existing BOM because it encodes exactly the replacement string. Vega retains existing UTF-8/UTF-16LE BOM so subsequent Read still recognizes the encoding. Main-agent approval 2026-09-22; this intentional correction is not byte-for-byte source parity. Read versions retain only SHA256 digests, not file contents.

### Encoding-aware write audit extension
`write_edit_v1` Write audits may include `expected_written_bytes` (strict u64, absent for legacy/input-equals-output). It binds the actual encoded byte count; content_bytes remains UTF-8 argument size. When present, fingerprint input uses tool domain `write:encoded` and additionally length-encodes expected_written_bytes as 8 big-endian bytes. Restore matches success bytes against this field or legacy content_bytes. Null/wrong type are invalid.

Checkpoint protection in §5 means preventing mutation of checkpoint storage and ensuring artifact confinement. Ordinary external Read of a checkpoint file uses normal permission rules (including Full access); it does not require another read blacklist.

Completed-call replay reconstructs input identity without reading the current file contents, without requiring the target to still exist, and without consulting fresh read state. Its prior encoding-aware byte count is part of identity; replay never grants mutation capability. Current symlink identity changes still cause conflicts.

LIMIT: Dangling symlinks are rejected until their target exists; existing symlinks resolve to their actual target. This fail-closed boundary avoids authorizing an unresolved identity and is intentionally retained.

Git control-component rejection is ASCII-case-insensitive (`.git`/`.GIT`) to preserve protection on macOS case-insensitive volumes.
