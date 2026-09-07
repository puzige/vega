# R11 — Branch popup geometry

Spec added before implementation, 2026-09-06. Root native acceptance found a two-branch popup constrained to its approximately 46px trigger width, with a fixed 240px list surface leaving blank space.

The popup shall use an explicit 320px width independent of the trigger, bounded by the window with 8px margins and anchored above/below the trigger according to the existing menu_below setting. Short lists shall use row-count height plus the existing failure banner and border; empty/loading/error status uses one row. The existing 240px maximum list surface and uniform-list virtualization/scrolling remain. Geometry only: no action, focus, operation, fence, Git or persistence changes. Verify formatting, affected build and existing branch selector/controller tests; root owns native small-window/light/dark validation.

Read-only sidebar finding: projects_block.rs renders projects.git_default_branch. Project registration initializes this field using git_detect::detect_git, which reads the then-current HEAD; it does not discover a repository default branch. Branch completion refreshes the selector and workspace, but does not update this registration-time field. Consequently the suffix is cached metadata and can look stale after checkout. Sidebar correction is outside this layout patch.

Verification: `cargo test -p vega -p vega_ui branch_` passed (8 app + 4 UI); `cargo build -p vega`, `cargo clippy -p vega_ui --all-targets -- -D warnings`, formatting and diff checks passed. Logs: `/private/tmp/vega-r11-branch-popup-{tests,build,clippy}.log`. Existing upstream `block` future-compatibility advisory remains. Native acceptance is delegated to root. No spec deviation.
