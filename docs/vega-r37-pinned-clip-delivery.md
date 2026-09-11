# R37 delivery

R36's native acceptance was incorrect: both scroll ancestors clipped the
leading surface despite correct row layout coordinates. R37 expands those
viewports while compensating their content padding, preserving title alignment.

## Evidence

- `cargo fmt --all -- --check`: passed.
- Strict workspace Clippy: passed; raw log `/tmp/vega-r37-clippy.log`.
- Updated existing R36 regression: passed, including both clip containers.
- First workspace run: unrelated real PTY test failed with empty output;
  retained `/tmp/vega-r37-workspace.log`. Isolated retry passed.
- Complete workspace retry: passed; `/tmp/vega-r37-workspace-retry.log`.
- `cargo xtask package`: passed; `/tmp/vega-r37-package.log`.
- Installed app screenshot: selected Pinned row now has a complete left
  rounded edge and visible leading padding. Title column remains aligned.
- Installed binary SHA256:
  `e2a2ff44e77e6a5401bb6b78870ceba2db618f5bdf82905ada3259c9c49e18c7`.

Native screenshot checked in the current light appearance. Dark geometry is
covered by the existing regression, not a separate native screenshot.
Previous installation retained in `/tmp/vega-r37-install.ZSrpyb`.
No remote push performed.
