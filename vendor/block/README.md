# Vendored `block 0.1.6`

Production-only compatibility copy of Steven Sheldon's Rust interface for
Objective-C blocks, used by Vega's existing macOS dependencies. The root
`[patch.crates-io]` selects this same-name, same-version package for every
consumer. It is explicitly excluded from workspace membership and has no
production or development dependencies.

## Source and integrity

- Published package: <https://crates.io/crates/block/0.1.6>.
- Source archive: <https://static.crates.io/crates/block/block-0.1.6.crate>.
- Archive SHA-256: `0d8c1fef690941d3e7788d328517591fecc684c084084702d6ff1641e993699a`, matching the original registry lockfile entry.
- Upstream repository: <https://github.com/SSheldon/rust-block>.
- Release commit: `47178790cfc9d4a8b092051d8b413b78bd31254a` (`Bumped to version 0.1.6.`).
- Original source: <https://github.com/SSheldon/rust-block/blob/47178790cfc9d4a8b092051d8b413b78bd31254a/src/lib.rs>.
- Original `src/lib.rs` SHA-256: `eb31678adf63b53109d9b94eba23699fd5f9ebfdb950f6e1a57ad51bb6a146fa`.

The cached archive was checksum-verified before copying. Its source matches
the release commit byte-for-byte. No upstream release newer than `0.1.6` was
available when this correction was selected.

## Production correction

Only two production expressions differ after normalizing comments and
formatting:

1. Declare `_NSConcreteStackBlock` as `[*mut c_void; 32]`, not as the
   uninhabited private `Class` enum.
2. Initialize `isa` with `ptr::addr_of!(_NSConcreteStackBlock).cast()`, taking
   the same symbol address without constructing an uninhabited reference.

The representation follows
[LLVM BlocksRuntime's native declaration](https://github.com/llvm/llvm-project/blob/main/compiler-rt/lib/BlocksRuntime/data.c):
`void * _NSConcreteStackBlock[32] = { 0 };`.

The private pointer-marker type, public API, callback signatures, block
layout, allocation, copying, disposal, target-specific library links, and
empty feature set are preserved. Edition 2015 is explicit to retain the
published package's implicit edition. No diagnostic is suppressed.

## Packaging omissions

This is not an unmodified redistribution of the complete archive:

- Omit the `#[cfg(test)] mod test_utils` declaration, six upstream unit
  tests and their module, `src/test_utils.rs`, and the `objc_test_utils`
  development dependency and its path wiring.
- Omit all Rust documentation and ordinary comments, including the two
  executable documentation examples. Replace the original example-based
  README with this provenance record; no Rust doctests are retained.
- Omit upstream manifest exclusions for absent CI scripts and iOS test
  material. Keep only the production library target, with doctests disabled.
- Apply whitespace formatting and optional trailing commas without changing
  production tokens beyond the two corrections above. Preserve implicit
  `extern` ABIs using rustfmt's `force_explicit_abi=false` option rather than
  introducing unrelated legacy warning fixes.

The complete vendored file inventory is `Cargo.toml`, `src/lib.rs`,
`README.md`, and `LICENSE-MIT`. It contains no test target, fixture, helper,
mock, development dependency, or test-only feature. Inventory this excluded
directory directly; workspace Nextest discovery alone cannot establish that.

## Licensing and attribution

The published `Cargo.toml` names **Steven Sheldon** as author and declares
`license = "MIT"`. The archive contains no standalone license file,
copyright notice, or copyright date. `LICENSE-MIT` supplies standard MIT
terms and the verified author attribution with the user's explicit approval;
it is not represented as an original upstream license file. No copyright
notice or date has been invented.

## Validation boundary

This change is a compiler/dependency-compatibility correction, not a new Vega
runtime behavior. Local verification is restricted to fast static source and
manifest comparisons, formatting, diffs, and offline locked Cargo metadata.
Vega code, builds, tests, Clippy, and Nextest are not run locally.

Pre-fix [PR Actions run 39526954240](https://github.com/puzige/vega/actions/runs/39526954240)
was reported by the coordinator to contain the `block v0.1.6`
future-incompatibility warning in both Clippy and Nextest. The source matches
Rust's `uninhabited_static` lint, but a detailed native Vega compiler report
was not captured: the diagnostic-only workflow change could not be pushed
because the OAuth credential lacks GitHub `workflow` scope. The workflow is
unchanged, and no permission workaround is attempted.

Fixed-source compilation, linking, and absence of the target diagnostic
remain for cloud PR verification. Require active resolution to this copy,
explicit `block v0.1.6 (.../vendor/block)` compilation evidence or matching
source fingerprints, and inspection of complete Clippy and Nextest logs.
A green gate or missing summary banner alone is not proof: path dependencies
can have different diagnostic handling. The native detailed-report limitation
remains explicit. Ordinary warnings from upstream implicit `extern` ABIs may
become visible for the local path package; do not suppress them or make
speculative cleanups without cloud evidence.
