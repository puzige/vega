# Default-on Skill list validation

## Revised acceptance

The user replaced per-item review with default-on switches and then selected a vertical list (icon, name, single-line description, real source, switch), not a card grid. Enabled sources and global/project automatic-selection gates remain authoritative. Existing disabled preferences stay disabled. Metadata schema is unchanged; nested values remain incompatible with the string-valued metadata map.

## Development evidence

- Base: `d40a08d5c435ca443b19eaf323b72831f47f967a`.
- Initial development: `bab3abd6bf20c5593116f79fedf0eb7ad6627725`, [Actions 41950804873](https://github.com/puzige/vega/actions/runs/41950804873): six Rust tests executed/passed, zero skipped; 60 Python passed. Clippy failed on unused deleted-panel imports and a collapsible if, not behavioral assertions.
- Green development: `44f4ef9f0f79a3b9152d757ddf115d2976af8d1a`, [Actions 41952069002](https://github.com/puzige/vega/actions/runs/41952069002): **8 discovered / 8 executed / 8 passed / 0 skipped**; 60 Python passed; Clippy and required aggregate check passed.
- Remote evidence tag: `evidence/644-toggle-development-44f4ef9f`, verified to resolve to that exact green development commit.
- Recoverable temporary test sources at that commit:
  - [`toggle_development_tests.rs`](https://github.com/puzige/vega/blob/44f4ef9f0f79a3b9152d757ddf115d2976af8d1a/crates/vega_conversation/src/agent/skills/toggle_development_tests.rs), 439 lines, SHA-256 `736b727f1105a091d5d893951eb5b453cb7a90251f17299c898ec492d7369a10`.
  - [`list_development_tests.rs`](https://github.com/puzige/vega/blob/44f4ef9f0f79a3b9152d757ddf115d2976af8d1a/crates/vega_ui/src/settings/skills/list_development_tests.rs), 71 lines, SHA-256 `92f6b6692bb903840dde76dd4748ff5a275faa34801f743005eac2d960f0e7d6`.

Coverage: no-review linked discovery → mention validation → activation without database approval writes; first disable persists and revokes, stale CAS/mentions rejected, new runs refuse explicit activation; same-content retarget rejects old drafts/pins while active frozen body remains unchanged; both automatic gates required; metadata/internal-symlink/hardlink rejection; distinguish same-basename imported directories; shadow feedback for default-on/content-updated winners.

Independent review of the initial delta identified two UI issues and a weak disable assertion. The green revision fixes them, strengthens the assertion without removing existing guards, and adds two UI projection regressions. Incremental independent review reported no new findings. These are static conclusions, not native UI acceptance.

## Final cleanup and limits

Final cleanup removes the 510 temporary test lines and six exclusive wiring lines, adding only this evidence document. No dependency, workflow, or Python test changes are needed. Exact cleanup diff and final-head required CI must pass independently before merge. Final Rust 0 discovered / 0 executed is a delivery cleanliness check, **not** a behavioral regression result.

Local checks: format, diff whitespace, locale TOML parsing only. No local Vega execution, compilation, builds, or Rust tests. Search layout, focus, keyboard and accessibility behavior have not been native-tested; user acceptance remains separate. No user configuration, database, Skill source, or installed application was changed. #644 remains open pending actual acceptance. Merge, release, and installation must be reported separately.
