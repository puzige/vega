# R39 delivery

Search now uses a neutral magnifier button in the existing top toolbar.
The previous full-width search row is removed, reclaiming its space.

- Native installed-app click opens the existing search palette.
- Production window test verifies button click, Escape and Command-K.
- Layout regression covers Light/Dark and minimum sidebar width.
- fmt and strict workspace Clippy passed.
- Workspace run passed preceding crates, then exposed obsolete R31 selectors
  for the removed search label/shortcut/row. Those assertions were updated;
  affected UI rerun passed 184 tests, xtask passed 36, all workspace doctests
  passed. Initial failures and final results retained under
  `/tmp/vega-r39-checks.AYxgu2`.
- Package and installed signature verification passed.
- Installed binary SHA256:
  `022728aea753984ec0867e80f9d094211d73ea274509fa4562b41aead922d4ac`.

Native shortcut interaction was not separately confirmed because the user
was operating the app; production automated coverage verifies Command-K.
Previous app retained under `/tmp/vega-r39-install.XMMzLw`.
No remote push performed.
