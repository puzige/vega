# Remove the `block 0.1.6` future-incompatibility warning

## Scope and baseline

- Issue: <https://github.com/puzige/vega/issues/509>.
- Baseline: `edc944040b66cfaf196272a6d5577f5c8fde4d70`.
- Branch: `feat/509-block-future-compat`.
- Plan version: 4. Version 3 preceded source implementation; version 4 records cloud-exposed implicit-ABI warnings before a semantics-preserving explicit-C spelling correction. Warning-removal acceptance is unchanged.
- Goal: fix the dependency source responsible for Cargo's future-incompatibility warning, without hiding diagnostics or changing Vega's runtime behavior, UI, storage, credentials, or public interfaces.
- Non-goals: upgrading GPUI/the macOS dependency stack, migrating all consumers to `block2`, changing unrelated package versions, or changing the existing CI topology.

## Evidence

The successful pre-fix [Actions run 39415626514](https://github.com/puzige/vega/actions/runs/39415626514), head `f4a1e0d9b5276ed8eea79f0bb783486d42b819e4`, emits the exact issue warning in both Clippy and Nextest:

```text
warning: the following packages contain code that will be rejected by a future version of Rust: block v0.1.6
```

The corresponding job IDs are `118603802522` (Clippy) and `118603802761` (Nextest). Passing `-D warnings` to workspace Clippy does not reject this dependency-level Cargo warning, so an ordinary green required check is not sufficient acceptance evidence by itself.

`Cargo.lock` identifies the registry package by checksum `0d8c1fef690941d3e7788d328517591fecc684c084084702d6ff1641e993699a`. Cached registry metadata lists no release newer than `0.1.6`. The coordinator's initial crates.io API request returned HTTP 403; the implementation Agent later successfully verified the same latest-version result from crates.io. Upstream [issue 21](https://github.com/SSheldon/rust-block/issues/21) reports the same warning, and [issue 22](https://github.com/SSheldon/rust-block/issues/22) asks about maintenance.

Direct lockfile consumers include Cocoa/Cocoa Foundation, Core Graphics/Core Video, GPUI's Apple/macOS layers, Metal, objc-foundation, and screencapturekit-sys. Replacing only one consumer would leave other paths to the same package.

The cached `block 0.1.6` source declares an empty `enum Class { }`, then `static _NSConcreteStackBlock: Class` in its foreign block. Rust's [uninhabited_static lint reference](https://doc.rust-lang.org/rustc/lints/listing/deny-by-default.html#uninhabited-static) documents this invalid pattern and its future-incompatibility diagnostic. The exact cached/cloud compiler report must be collected if available; static source analysis alone is not reported as an executed reproduction.

## Acceptance cases (frozen before implementation)

| Case | Observable result | Verification |
| --- | --- | --- |
| Complete workspace dependency compilation on pinned Rust 1.98.0 | No target future-incompatibility diagnostic for the actively resolved `block` package in Clippy or Nextest output, regardless of version | Existing cloud PR gate; inspect full job logs and native dependency diagnostics, confirming corrected source was compiled or has matching source fingerprints |
| Existing Objective-C block consumers compile/link | Existing `block` API, callback signatures, block layout, copy/dispose behavior, and dependency compatibility remain intact | Minimal source diff review and existing cloud all-target Clippy/Nextest build |
| Warning visibility | No `allow`/`cap-lints`/report-frequency setting, stderr filtering, or other warning suppression is introduced | Static review of all changed manifests, source, Cargo config and workflows |
| Dependency scope | No unrelated package addition, removal, version change, or source migration; every affected path resolves to the corrected package | Parsed lockfile/resolved metadata comparison and source review; no unused patch or active unpatched duplicate |
| Delivery gate | Existing fmt, all-target Clippy, Python checks, and Nextest inventory/execution succeed on the exact final head | Required `check (fmt, clippy, test)` plus `no Rust tests` (0 discovered/0 executed) evidence |
| Reproducibility and licensing | The local copy is included in the repository, references checksum-verified upstream content, includes standard MIT terms and verified author attribution without inventing a copyright notice/date, and adds no dependency | Source checksum, licensing and manifest review; user explicitly approved this transparent licensing treatment in the current conversation |

This is a compiler/dependency-compatibility task, not a new Vega product behavior. Do not manufacture a runtime assertion or a source-string test that cannot observe the compiler warning. Preserve the pre-fix raw compiler warning and compare it to the fixed cloud build output. Any implementation that changes runtime behavior or public API requires stopping and opening a behavior TEST-FIRST cycle before that change. Final tree must not introduce Rust tests or restore historical tests.

## Implementation plan

1. Dedicated implementation Agent confirms the exact diagnostic, compatible upstream options, minimal source representation fix, and licensing/provenance requirements before writing code. Independent reviewer checks verification and scope.
2. No compatible published fix was found; use a reproducible, narrowly scoped replacement of the existing `block 0.1.6` package, with no added dependency. Do not substitute an incompatible `block2` API. The user explicitly selected vendoring with standard MIT terms and verified author attribution after disclosure that the published package has no license file; document that limitation and do not synthesize an upstream copyright notice/date.
3. If vendoring is necessary, place only required production source/manifest and license/provenance under a clearly named directory; exclude it from workspace membership. Preserve upstream production logic except two FFI corrections: declare `_NSConcreteStackBlock` as `[*mut c_void; 32]` and initialize `isa` using `ptr::addr_of!(_NSConcreteStackBlock).cast()`. This matches the native Blocks runtime's `void * _NSConcreteStackBlock[32]` definition and takes the same symbol address without constructing a reference to an uninhabited value. The private pointer-marker type, block layout, callback signatures, allocation/copy/disposal and public API remain unchanged. See [LLVM BlocksRuntime data.c](https://github.com/llvm/llvm-project/blob/b8665226ec21e500dd210a39b8ca6c99dd7f4f51/compiler-rt/lib/BlocksRuntime/data.c#L33) for the native declaration. Preserve upstream edition 2015 semantics, production features/link behavior and zero production dependencies. Do not copy upstream tests, test-only support/dev dependencies, or executable doctests into the final tree; inventory the excluded vendor directory explicitly rather than relying only on workspace Nextest discovery. Enumerate these packaging omissions separately from the two production corrections. Follow repository no-comment policy; keep provenance and required license text in documentation/license files, not new code comments.
4. Patch all crates.io consumers through the root manifest, update only the corresponding lockfile source identity, and statically confirm resolution to the checked-in source. Keep package identity/version and all existing consumers compatible.
5. Parent coordinates Git/PR and independent review. Existing cloud PR checks provide compilation/testing; local work is limited to fast static/diff/format/metadata checks. No local Vega code execution, compilation, tests, Clippy, or packaging. An attempted diagnostic-only `--future-incompat-report` addition was rejected on push because the OAuth credential lacks GitHub `workflow` scope. Remove that unpublished workflow change; do not bypass permission or change credentials/protocol. The workflow remains byte-for-byte unchanged. Native detailed-report capture is therefore unavailable in this task, not claimed as completed.
6. Inspect complete compiler diagnostics from both final cloud jobs rather than assuming the required check or disappearance of the summary banner proves absence. Require explicit fixed-source `block v0.1.6 (.../vendor/block)` compilation evidence, active patch selection, no target diagnostic in full logs, and independent comparison of the retained production source with the checksum-verified upstream archive. Changing registry code to a path package can change diagnostic handling, so banner absence alone is not accepted. The native detailed report limitation above remains explicit. Query running CI no more often than every five minutes.
7. Squash merge only after independent review and the exact final head's required check pass. Leave Issue open and move it to In review for user confirmation; merge is not application installation or user acceptance.

## Rollback and handoff

Rollback reverts this task's package override, checked-in compatibility source and matching lockfile entry together; unrelated dependencies and application state remain untouched. The original warning will return if the original registry package is restored before an upstream fix exists.

Parent owns this spec, Git integration, external issue/project/PR state and CI review. Implementation Agent owns only explicitly delegated dependency files after approval. Independent review is read-only. Record exact commits, static commands/results, cloud run/job URLs and limits in the PR. Runtime RED/GREEN/delete-test evidence is not claimed for this compiler-only correction.

## Cloud-exposed compatibility follow-up

Implementation head `80a863f65b684352148ffba93f9a61c2816e3bea` passed [run 39529303170](https://github.com/puzige/vega/actions/runs/39529303170). Both jobs explicitly compiled/checked the vendored path and the complete logs contained zero target future-incompatibility diagnostics. The source-level static correction is therefore supported by actual cloud compilation, not merely a missing summary banner.

Those same logs exposed 46 `missing_abi` warnings per compilation, previously hidden for the registry package. All point to upstream implicit `extern` syntax, which already means C ABI. Before further edits, freeze the narrow follow-up: spell every existing implicit ABI as `extern "C"` in the vendored production source, including macro-generated function-pointer/callback forms and the foreign block. This does not change a calling convention, public contract, block layout, callback behavior or dependency. Do not suppress warnings. Update provenance to enumerate this evidence-backed spelling correction separately from the two semantic FFI edits; pin the native reference. Runtime TEST-FIRST remains N/A. Re-run static checks and the complete unchanged cloud gate on the resulting exact head; require no vendored compiler warnings as well as no target future-incompatibility diagnostic. This is not a relaxed assertion, new runtime behavior, or a post-hoc runtime test.

## Validation status

- Pre-fix warning: confirmed in pre-fix PR run 39526954240 and the earlier linked run.
- Exact diagnostic: static source/lint match to `uninhabited_static`; detailed Vega compiler report unavailable because workflow writes were denied. No raw detailed report is claimed.
- Initial implementation and independent review: complete at `80a863f65b684352148ffba93f9a61c2816e3bea`; reviewer approved with a nonblocking immutable-reference suggestion.
- Local static checks: workspace fmt, vendored rustfmt, diff/whitespace, TOML/lockfile/source/archive and locked offline resolved metadata passed; no local Vega build/test executed.
- Initial cloud gate: all jobs passed; Python 5 + 15 + 14 tests passed; Rust `no Rust tests` (0 discovered/0 executed). Target warning absent; explicit-ABI warning follow-up still required.
- Final explicit-ABI head review and cloud gate: pending.
