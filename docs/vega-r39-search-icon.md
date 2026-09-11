# R39 Sidebar search icon

Replace the full-width Search navigation row with a compact magnifying-glass
button in the existing Sidebar top toolbar, alongside its existing controls.
Use the shared outline Search icon and neutral styling. This reference does
not request adding a brand header or notification button.

Click and keyboard activation open the same existing command/search palette.
Keep Command-K working. Provide an accessible Search name and shortcut hint.
Remove the old search row and its reserved space so organization sections move
up naturally. Preserve New Task and all organization content geometry.

Update obsolete R34 tests that require a full-width Search row and the R31
typography assertions for its removed row, label and shortcut to match this
new contract; avoid redundant cosmetic test coverage. Verify actual button
activation, Command-K, and native layout. Required fmt, Clippy and workspace
tests must pass. Use existing icons/components; no dependencies or new tokens.
