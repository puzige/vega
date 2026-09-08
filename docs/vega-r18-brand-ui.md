# R18 — Vega 品牌 UI 与 Icon 基线

Date: 2026-09-08
Status: Implemented on `feat/r18-brand-icons`
Scope: shared native SVG icons, theme tokens, PI sidebar and composer emphasis states.

R18 adopts the approved R17 App Logo as the visual source for product chrome. The
translation is a restrained UI language: the blue terminal mark, one slender
spark, a shallow smile curve, soft corners and quiet light/dark depth. The logo
is a reference for geometry and emphasis, not a decorative stamp applied to
every control.

## Foundations

- **Optical grid:** every shared icon is placed in a fixed 16 × 16 container.
  The bundled Lucide-style assets keep their 24 × 24 viewBox and are rasterized
  by GPUI's SVG renderer, so Retina scaling does not depend on fractional
  `PathBuilder` coordinates.
- **Stroke and geometry:** the source icons use a 2px stroke at 24px with
  round caps and joins, producing a consistent approximately 1.4px optical
  line at the 16px UI size. Chevron, plus, more, settings, mode and panel
  controls all use the same mature outline language.
- **Folder:** Folder uses the standard GPUI Kit folder outline. FolderPlus uses
  one standard Lucide folder-plus SVG so its add affordance stays legible in a
  fixed 16px container; no smile, star or custom facial detail is added to a
  functional folder.
- **Brand spark:** only Agent/AI/Thinking actions may use the spark-like
  `Asterisk` asset. The App Logo remains the source of the actual brand mark;
  generic controls retain their familiar functional silhouettes.

### Shared icon source

`vega_ui::icons` maps its stable `Icon` API to `gpui_kit::component::IconName`.
The `gpui-kit-assets` bundle supplies the layout, action, file, folder,
disclosure, arrow, settings and AI symbols as embedded Lucide-style SVGs.
`IconName` elements always render at `px(16.)` and are colorized by the caller's
semantic theme token. FolderPlus, Pin and Shield are the only bundled-set gaps;
their static Lucide paths go through GPUI's `svg().data(...)` renderer with the
same viewBox, stroke, cap and join rules. No new runtime asset loader or
dependency is introduced.

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

R18 keeps icon semantics in the shared local API, uses no Unicode glyph as an
interactive icon, and routes all generic geometry through the SVG renderer.
The only custom geometry permitted here is the approved App Logo itself;
functional symbols are not hand-drawn with `PathBuilder`. R15 project/session
IA, storage, navigation, providers, Keychain policy, business behavior and
accessibility contracts remain unchanged. The App Logo source files are not
modified by this baseline.
