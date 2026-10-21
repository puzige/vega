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
- Vendored production `src/lib.rs` SHA-256: `f7c2001a86b40340fcc82b18fbb624d6ab0295956f3178256c0cd83619d0f67d`.

The cached archive was checksum-verified before copying. Its source matches
the release commit byte-for-byte. No upstream release newer than `0.1.6` was
available when this correction was selected.

## Production corrections

Two FFI corrections replace the invalid source representation:

1. Declare `_NSConcreteStackBlock` as `[*mut c_void; 32]`, not as the
   uninhabited private `Class` enum.
2. Initialize `isa` with `ptr::addr_of!(_NSConcreteStackBlock).cast()`, taking
   the same symbol address without constructing an uninhabited reference.

The representation follows
[LLVM BlocksRuntime's native declaration](https://github.com/llvm/llvm-project/blob/b8665226ec21e500dd210a39b8ca6c99dd7f4f51/compiler-rt/lib/BlocksRuntime/data.c#L33):
`void * _NSConcreteStackBlock[32] = { 0 };`.

Separately, every upstream implicit `extern` is spelled `extern "C"`,
including the foreign block, function-pointer types, and macro-generated
callbacks. This preserves the existing implicit C calling convention.
[Initial cloud run 39529303170](https://github.com/puzige/vega/actions/runs/39529303170)
on `80a863f65b684352148ffba93f9a61c2816e3bea` exposed 46 `missing_abi`
warnings per compilation at ten source sites. The explicit spelling corrects
those evidence-backed deprecations without changing behavior or suppressing
diagnostics.

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
  production tokens beyond the two FFI corrections and explicit C ABI spelling
  above. Default rustfmt formatting is used.

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

The initial cloud run linked above passed all jobs. Both Clippy and Nextest
explicitly compiled or checked `block v0.1.6 (.../vendor/block)`; their
complete logs contained no target future-incompatibility diagnostic, but did
contain the implicit-ABI warnings described above.

This explicit-ABI revision still requires independent review and the full
unchanged cloud gate on its exact head. Require active resolution to this
copy, explicit vendored-source compilation evidence or matching source
fingerprints, and complete Clippy and Nextest logs with no vendored compiler
warnings or target future-incompatibility diagnostic. A green gate or missing
summary banner alone is not proof: path dependencies can have different
diagnostic handling. The native detailed-report limitation remains explicit.
