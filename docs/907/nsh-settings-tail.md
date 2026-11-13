# #907 Settings visible-path tail

## Scope and acceptance

Current authorization: finish concrete live Settings visual residue only; no intermediate independent review. Worktree `/Users/puzige/Workspace/worktrees/vega-907-nsh-full`, supplied dirty HEAD `0b5ea426abfd8d4358489e916273d89474fb190f`, supplied fetched ancestor `34555cec9c0c4c4c1f7a528d739752f414e53edf`. These Git facts are supplied by the parent, not independently queried here. No Git/network/Cargo/rustc/application/delegation commands are authorized in this leaf.

Actual production path: `crates/vega/src/window/render.rs:467` mounts `SettingsView`, whose `Render` is `crates/vega_ui/src/settings/render_impl.rs`. Its missing-view fallback is an empty layout, not a demonstrated old background defect. `settings/nsh_settings.rs` is not declared in `settings/mod.rs` and is not the production renderer.

Concrete live residue found in `render_impl.rs`: two `bg_base` GPUI fills; Defaults invokes `settings_group`, `section_title`, and two `settings_field` calls. Their live GPUI pixels are the group fill/rounded border, title, field labels and bottom rules. Existing child permission/default-model controls are NSH and must stay intact.

Replace only these pixels using existing `crate::nsh::Surface::new_transparent`; keep layout dimensions, padding, wrapping, border space, scroll handles, child controllers and action/focus/localization/security authority unchanged. Passive backgrounds must not participate in Tab, pointer/wheel handling or accessibility. Labels retain accessible text and natural measured height; stale measurements must be rejected after content/font/width changes. Empty layout hosts, dead helpers and controller entities are not migration defects.

## Original file identities

SHA-256 before this leaf's changes (including all pre-existing parent work):

- `settings/mod.rs`: `d09d7c1cfd1b846b2d3f4d2e2a73cff8068d0292e32a9de7a30feec907448f0e`
- `settings/helpers.rs`: `3bbd5a99783368b855d9aca54d74d3ac052f7b9655f6a28b15fa0e167b662d17`
- `settings/render_impl.rs`: `7388e9fb7baa4fa38997ccef11e4fa6a6de362eb08f0b1a01a1222e8d01ebb5a`
- `settings/nsh_tail.rs` and this document did not exist.

## Implemented exact parent delta

Only five paths were written by this leaf:

- `crates/vega_ui/src/settings/mod.rs`: +1/-0, private `mod nsh_tail;` only.
- `crates/vega_ui/src/settings/helpers.rs`: +45/-8, pass the existing host context into the three live helpers, replace GPUI label/description/title children and group/rule pixels with the private adapter, retain transparent original border space and original layout tokens. Two fields have distinct identities; labels are mounted through deferred container queries under their respective field IDs.
- `crates/vega_ui/src/settings/render_impl.rs`: +23/-3, update those helper calls and replace the two explicit page/content fills with absolute passive NSH backgrounds.
- `crates/vega_ui/src/settings/nsh_tail.rs`: new 403-line private adapter using the existing Surface; no business commands. Background/rule/group pixels are AX-hidden and non-Tab. Passive prepaint masks prevent AppView pointer/wheel hitboxes; paint retains the actual parent clip. Text retains accessible labels, inherited font/fallbacks/features, existing typography/color tokens and natural measured height. Content/font/width changes stop the old Surface, rotate ProjectionId and reject stale/invalid height reports. Owner release stops the child Surface. The only `try_send` is an idempotent layout measurement, not a user action.
- This document: new scoped evidence record.

The modified existing files' exact parent-dirty preimages were reconstructed in memory from the inspected original snippets and verified against all three original SHA-256 values above before generating a Python unified diff. Thus the counts describe this leaf's changes, not the complete diff of the dirty worktree against HEAD. No Git command was used. No state/controller/shared Surface/dependency/vendor/shell/disclosure/footer/link/body files were written.

Final production-source SHA-256:

| File below `crates/vega_ui/src/settings/` | SHA-256 |
|---|---|
| `mod.rs` | `fed923f0de45180285f5d35296c6dcd8d841d0cee818260c4357d545853de7b4` |
| `helpers.rs` | `e3c02d4921f816cbc065585a2cfa71f9abcdec2b902bf40f7b9463c5bac600a0` |
| `render_impl.rs` | `98f25a61151d6a4bfb477b1f6bfae06591d8cee766ff49b370ce328ac1efba29` |
| `nsh_tail.rs` | `4da41e54455d0ad9bf1c2bd64c1a14ae3b61215eda9a47ad8a292aca61855864` |

## Inspected live paths and scope conclusion

Read actual Render dispatch and follow only its production branches: `nsh_shell.rs`; `model_configuration/nsh_navigation.rs`; `provider_management.rs` to `provider_management/nsh_providers.rs`, `model_capabilities/nsh_capabilities.rs`, `model_catalog/nsh_catalog.rs`; `reasoning_render.rs` to capability efforts; `nsh_pricing.rs`; `usage/nsh_usage.rs`; `mcp/nsh_mcp.rs`; `skills/nsh_skills.rs`; `agents/nsh_agents.rs`; General's `nsh_appearance.rs`, `permissions/nsh_permission.rs`, `permission_reset.rs`, `nsh_default_model.rs`, `nsh_input_preferences.rs`, `updater/nsh_update.rs`. The section scrollbar/fade remains the existing `ScrollEdgeFadeExt` adapter. Existing empty div fallbacks and GPUI input entities used only as controller buffers were not changed. Other leaf controllers and their delayed action owner checks are byte-untouched by this leaf.

The concrete outer/Defaults residue found above has been replaced at source level. This is a scoped implementation conclusion, not proof of full migration, behavior equivalence, native acceptance or cross-platform rendering.

## Actual checks

- Ran `/Users/puzige/.rustup/toolchains/1.98.0-aarch64-apple-darwin/bin/rustfmt --version`: `rustfmt 1.9.0-stable (88d9e12ae1 2026-08-18)`.
- Ran that binary with `--edition 2024 --config skip_children=true` on `nsh_tail.rs`, `helpers.rs`, `render_impl.rs`; then the same three-file command with `--check`: exit 0. Existing `mod.rs` otherwise retains the parent's formatting.
- Read-only Python whitespace/final-newline inspection on all four source files: no trailing whitespace, all final newlines present.
- New-file policy scan: 0 ordinary comments, unwrap/expect, unsafe, test attributes or cfg(test). No Rust test or test wiring added.
- Focused source scan of `render_impl.rs`/`helpers.rs`: no remaining `.bg(...)` or raw label/description child; only two transparent GPUI border colors remain as layout space. This scan is not a runtime regression test.
- Read pinned local NSH 0.4 builder/dynamic sources and GPUI Fast 0.1.9 keyed-state API; reused the current production `conversation_status.rs` measurement/passive pattern and `window/nsh_workspace/separator.rs` passive background pattern. One guessed registry directory name for `nsh-platform-gpui` did not exist; no tool was installed and no registry changed. AppView stop and Surface APIs were inspected through existing source callers.
- Git/network/Cargo/rustc/app/delegation/independent review were not run, per this leaf's authorization. No compilation, Clippy, tests, CI or native interaction pass is claimed.

## Parent commands and pending verification (not executed here)

From the worktree root, parent should inspect the five-path delta against its retained dirty preimage, then run central validation on the integrated final source, including:

```sh
cargo check -p vega_ui -p vega --locked --offline -j 2
cargo clippy -p vega_ui -p vega --all-targets --locked --offline -j 2 -- -D warnings
```

Use the parent's already selected toolchain/cache/concurrency setup; the commands above are requested checks, not successful results. Parent owns current-head Git/CI verification and any required final delivery gates. Native pending checks: light/dark and EN/zh-CN labels, narrow wrapping/natural height, opening/closing model and permission controls with error/recovery and reset confirmation, cross-Surface Tab/Shift+Tab/focus restoration, wheel scrolling through passive labels/backgrounds, section scroll restoration, theme/language/resize changes with delayed height reports, and accessibility names/no decorative nodes. No intermediate independent review was requested or started. Stop writing after READY.
