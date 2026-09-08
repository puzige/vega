# R18 — Vega 品牌 UI 与 Icon 基线

Date: 2026-09-08
Status: Implemented on `feat/r18-brand-icons`
Scope: shared native vector icons, theme tokens, PI sidebar and composer emphasis states.

R18 adopts the approved R17 App Logo as the visual source for product chrome. The
translation is a restrained UI language: the blue terminal mark, one slender
spark, a shallow smile curve, soft corners and quiet light/dark depth. The logo
is a reference for geometry and emphasis, not a decorative stamp applied to
every control.

## Foundations

- **Optical grid:** every shared icon is authored on a 16 × 16 optical grid.
  Strokes are centered on the grid and leave enough breathing room at 1px
  rendering sizes. The usable drawing area is generally 12 × 12 rather than a
  mathematically full box.
- **Stroke and geometry:** the default stroke is 1.45px, within the R18
  1.4–1.5px range. Endpoints and joins use shallow curves or explicit curve
  segments. Chevron, plus, more, settings, mode and panel controls share this
  compact, rounded language.
- **Folder:** Folder and FolderPlus use a dedicated soft folder outline. The
  tab and outer frame stay simple; the bottom edge has only a very small upward
  bow so it reads as a folder silhouette, never a face. A normal folder does
  not contain a star.
- **Spark:** the single slender four-point spark is reserved for Agent/AI,
  Thinking and other key brand actions. Functional meaning remains with the
  functional icon beside it.

## Color tokens

The logo sapphire is the brand accent while the workspace stays neutral. The
light palette uses primary `#3478D8` and strong `#245AAF`; the dark palette
uses ice blue `#8FC7FF` and strong `#609DE1`. `accent` aliases the appearance's
primary brand color, and `brand_on_accent` is used for content rendered on a
primary button.

Selection uses a low-contrast brand wash: `#EAF2FC` in light mode and
`#203247` in dark mode. Selection text and selected functional icons use the
primary brand token so the state is visible without a heavy blue fill. Hover
continues to use the neutral hover token. `success`, `danger` and `warning`
remain semantic status colors and are not recolored as brand blue.

All component colors come from `vega_theme::ThemeColors`; components do not
introduce hex literals or appearance-specific colors.

## Product surfaces

- PI `PROJECTS` rows use a neutral folder in their ordinary state and the brand
  primary for the selected folder, with a soft brand selection background.
- PI `SESSIONS` and project task rows retain the R15 two-level IA and existing
  keyboard, focus, hover, archive and action behavior.
- Composer project context uses the folder vector and brand primary when bound
  to a project. Mode, model and thinking controls use the same selected-state
  treatment; permission `warning` remains warning-colored.
- The main send action uses `accent` with `brand_on_accent`; disabled send stays
  neutral. No gradient or full-surface blue treatment is introduced.

## Constraints

R18 keeps icons local vector/canvas paths, uses no Unicode glyph as an
interactive icon, and adds no runtime dependency. R15 project/session IA,
storage, navigation, providers, Keychain policy, business behavior and
accessibility contracts remain unchanged. The App Logo source files are not
modified by this baseline.
