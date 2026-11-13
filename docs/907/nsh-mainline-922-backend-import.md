# #907 / #922 exact backend import evidence

## Status and scope

**READY — bounded exact import and static identity checks only; stopped after handoff.** This is author/import evidence, not independent implementation review, native acceptance, compilation, test or CI evidence. The parent retains final integration and all remaining gates.

- Verified at UTC: `2026-11-09T10:56:52.237333+00:00`.
- Worktree HEAD (unchanged): `0b5ea426abfd8d4358489e916273d89474fb190f`. The migration is dirty; HEAD alone is not its content identity.
- OLD, previously integrated mainline: `0b3dcd3603a8e97e0ef74ae25a26f17fc3a094e5`; tree `06edf197b5963370499cc876cbe1f19c5c300717`.
- NEW, pinned import source / #922: `2a29adf9b2ea78eaf59d591838531b45adc7373e`; tree `a54ccd7591fd6886817dad7343634044119a28ab`; its first parent is OLD.
- Parent supplied the completed remote fetch and task-branch remote=head/no-update facts. This lane did not fetch, contact a remote or mutate Git/index/refs/commits.
- Read root AGENTS, exec-guide, the complete [compatibility inventory](nsh-mainline-922-compatibility.md), relevant current files, the complete 18-path OLD-to-NEW delta and all three added files. No applicable nested AGENTS was found along these paths.
- Sole write set: exactly the 12 inventory copy candidates below plus this new evidence document. The parent's six mixed UI/locale paths and `nsh_tool.rs` were not written.

## Prewrite fence and method

Immediately before any import write, all nine existing candidates were completely read as raw bytes and asserted equal to `git show OLD:path`. All three additions and this evidence path were asserted absent (including symlinks); target paths and ancestors were checked for symlinks. All pinned NEW blobs matched the inventory. The path-restricted binary patch passed `git apply --check -` without being applied by Git.

The write process loaded only `git show NEW:path` bytes into an explicit 12-path allowlist. It rechecked each existing file immediately before replacement and used exclusive creation for additions. It did not stash/reset/checkout/restore, stage, merge, format, or change modes. Readback compared complete raw bytes with the pinned source, then recomputed Git blob SHA-1 and SHA-256 for all 12 files.

Prewrite SHA-256 / absence (not HEAD/index state):

```text
77acacdd21e4283c8310d312a0ff67b6c69968be35e3eb067f51d0934a46364d  crates/vega_conversation/src/agent/shell_operations.rs
264c8148fd1816d1affbc17f83343ec4653e82c55b2440f8705dfe67c1032433  crates/vega_conversation/src/shell_recovery.rs
206ac0d280f133095f358fe357bf4a6f355902923393f7e8f119aa742af2f950  crates/vega_runtime/src/agent/mod.rs
0e6d73836e9f6d1883187701654e4f5e66c33782e5718aee0d953c5a252ca34b  crates/vega_runtime/src/agent/shell_sessions.rs
2d3878b320ad5f35d350adf30a5baeab802c62bf3c18d0c79a5a15117cf94849  crates/vega_runtime/src/agent/tools_exec.rs
082353359c84c9578885bb9b026455f1f7d43d686f9f75e342f3d4161adb610a  crates/vega_runtime/src/lib.rs
ABSENT  crates/vega_store/migrations/0063_shell_concurrency.sql
ec23a8663ccd428e75f9ae8bec0131331fd2f72dd52d2f04c4a03cc84cbdd4f3  crates/vega_store/src/lib.rs
563424011f99731ea567b5867146382bd21451addac6cc66a862a71baeb3cb95  crates/vega_store/src/shell_operations.rs
957b25e5a521abcd28763a8aabee07fee19f8b9fc5233ef1df5e4fb4d6d9aeb6  crates/vega_tools/src/bash/session.rs
ABSENT  docs/921/full-access-background.md
ABSENT  docs/921/implementation-evidence.md
```

## Exact imported identities

All 12 raw worktree files equal pinned NEW, byte for byte. These identities, not a moving remote ref, define this lane's imported content.

| Path | Git blob SHA-1 | SHA-256 | Bytes |
| --- | --- | --- | ---: |
| `crates/vega_conversation/src/agent/shell_operations.rs` | `db1722c234c50e2504072ed961c74bc34047cf90` | `dc858ccaeb3876de15e18c749966de8cb4f6299d25a17024104fd7de4d55a3a8` | 12399 |
| `crates/vega_conversation/src/shell_recovery.rs` | `13079f60f74d810784f86fe6fa3bb1273d0a66c7` | `036f7d983087145c0f4ae3f3d6a7c57919dc0ac2482efd9f0da7c6901b8619c3` | 22593 |
| `crates/vega_runtime/src/agent/mod.rs` | `19338661525b3233850878c4dbbb5bc5b8fb38ed` | `98ffaa3eaf9e551bfb84f569fe0bff00df03811bd4e15073bfa4b4e6b70f69bd` | 58354 |
| `crates/vega_runtime/src/agent/shell_sessions.rs` | `6a99a2f6433c03f5660b2f4e01349966c9b66308` | `4d91d598a989a47ae9af0e40d9782c1de0c5247acbbac0875f001a27b2e50e99` | 43419 |
| `crates/vega_runtime/src/agent/tools_exec.rs` | `a27d2010d08f616a813deaa5d4c1913f8deb0120` | `36f4c040f1a08963a3956552a03edecda143b9886e6be7632c6d1eca56a36bb8` | 111391 |
| `crates/vega_runtime/src/lib.rs` | `e384b3456ca5f70ac4388a6712139d3633cb6f33` | `b17d55338cd20d957e18027d646f1c172a6541b265c105406527df96026716c1` | 5501 |
| `crates/vega_store/migrations/0063_shell_concurrency.sql` | `dbf87a3554e5eecf1bfec4ff7b841567bc7d0ea9` | `d84ff4aa5f41fcbc958af574924797dccf8bef1d8094d816363f6b4b6163f803` | 108 |
| `crates/vega_store/src/lib.rs` | `cccd312127f68763d3f7e2d5f371194eb2770fb9` | `c95447cd16441403e6c47e5333831b6c902e082d2ea9aaa62a02c37c4d6809dc` | 12045 |
| `crates/vega_store/src/shell_operations.rs` | `800434b7686d1acd8cce6116099a6ccfc02005fd` | `0c12cf497b94cab0e7513fcc85df7c42640481c1d5267b577788c46c0652245d` | 24233 |
| `crates/vega_tools/src/bash/session.rs` | `03cdaea1de0a703ef16a6f261df4c7cb3a92b0a7` | `abb0cda6f8aed4e38e7c946492aedf6562106a4a42ecb641d56f95c8e9cc467f` | 31224 |
| `docs/921/full-access-background.md` | `380d6b51de0de92f72015e0e8a3cc07df3abe1c6` | `958ece766e71fe6a7209765ca9f3a0985e5817d908add5e08cb444e63315e0a5` | 3949 |
| `docs/921/implementation-evidence.md` | `0e462d268274f538e18546ba16549ad11c0d7ba5` | `bd7b11354e285f8fd6253ded3e31023cedadfe64874c8d4e4b2c1f2903f99eca` | 4587 |

## Static checks actually executed

| Check | Result |
| --- | --- |
| `git rev-parse HEAD` and `git rev-parse NEW^` | Exact HEAD and OLD as above |
| Scoped `git diff --binary OLD NEW -- <12 paths>` piped in memory to `git apply --check -` before writes | Exit 0; no patch application by Git |
| Complete prewrite raw-byte comparison | 9/9 equal OLD; 3/3 new candidates absent; evidence absent |
| Complete post-import raw-byte / expected blob / SHA-256 readback | 12/12 exact NEW |
| `git diff --no-ext-diff --no-textconv --check OLD -- <12 paths>` after import | Exit 0; tracked current import differences have no whitespace errors |
| `git diff --no-ext-diff --no-textconv --check OLD NEW -- <12 paths>` | Exit 0; also covers the three additions not yet tracked in this worktree |
| Migration registration static inspection | Exactly 63 unique sequential includes (0001–0063); every referenced SQL path exists; 0063 appears once immediately after 0062 |
| Migration content inspection | Exact pinned SQL: `concurrent INTEGER NOT NULL DEFAULT 0 CHECK (concurrent IN (0, 1))`; no database opened or migration executed |
| Added source-comment line scan of scoped OLD-to-NEW Rust/SQL delta | Zero added comment lines |
| Write boundary | Explicit file-write log contains exactly the 12 import paths and this evidence path; no other file-writing operation was executed |

Scoped pinned OLD-to-NEW diff summary:

```text
.../src/agent/shell_operations.rs                  |   1 +
 crates/vega_conversation/src/shell_recovery.rs     |  80 +++++++++++-
 crates/vega_runtime/src/agent/mod.rs               |   5 +-
 crates/vega_runtime/src/agent/shell_sessions.rs    | 144 +++++++++++++++------
 crates/vega_runtime/src/agent/tools_exec.rs        |  29 ++++-
 crates/vega_runtime/src/lib.rs                     |  19 +--
 .../migrations/0063_shell_concurrency.sql          |   1 +
 crates/vega_store/src/lib.rs                       |   1 +
 crates/vega_store/src/shell_operations.rs          |  73 ++++++++---
 crates/vega_tools/src/bash/session.rs              |   2 +-
 docs/921/full-access-background.md                 |  37 ++++++
 docs/921/implementation-evidence.md                |  31 +++++
 12 files changed, 349 insertions(+), 74 deletions(-)
```

## Comment provenance and preserved migration

Existing comments in imported files are preexisting mainline bytes; no ordinary source comment was authored or added by this import delta. Exact-source copying intentionally does not remove or rewrite upstream comments. This provenance is documentation, not a new code comment. The upstream `docs/921/implementation-evidence.md` is imported unchanged as historical #921/#922 evidence; none of its toolchain/test/review/CI results is claimed for current #907.

Earlier dirty #916/#920 backend content was preserved by the OLD-equality fence and NEW's direct-parent relationship. Other migration, dependency, vendor, UI and shared-file bytes were not written by this lane. The write log demonstrates this process's scope, not global worktree immutability: the parent was concurrently adapting its disjoint files, so no assertion is made that unrelated paths stayed unchanged globally.

## Limitations and next owner

- No Cargo/rustc/toolchain query, build, test, feature command, application/native UI execution, SQL execution, CI query, dependency/vendor operation, user-data access, network, child delegation or Git mutation was performed. Cargo/rustc versions and current CI/test counts were not obtained; unrun checks are not passes.
- Static SQL registration/content coherence is established; actual database migration and runtime behavior are not tested here.
- This import alone does not establish full #922 compatibility. Parent owns six mixed UI/locale adaptations plus the existing NSH card adapter, coherent final-version validation, independent review, required CI and user/native acceptance.
- No commit/merge/release is created or claimed. No unexpected candidate delta was observed and no force overwrite was used.

**READY — exact import lane complete; stop writing and return ownership to parent.**
