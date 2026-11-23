# Lucubra brand sheet

Lucubra is a local-first study engine for students. The name comes from "lucubrate", to study by lamplight. This sheet records the logo, colour, type and sizing rules for the Lucubra release. It follows the uncoalesced Mosaic system (`MOSAIC_SYSTEM.md`) and the token file `Branding/tokens.json`.

## Colour

Lucubra has one accent, Watermelon, and two grounds. Decision D52 sets Watermelon as the Lucubra accent.

| Token | Hex | Role |
|---|---|---|
| Graphite | `#333333` | Default ground for the mark and the UI dark surface |
| Porcelain | `#FDFFFC` | Light ground and dark-mode ink |
| Watermelon | `#EF2D56` | Accent. Fills and icons only |
| Shade | `#AD2F4A` | Watermelon mixed 35% toward Graphite. Mark ramp |
| Tint | `#F47690` | Watermelon mixed 35% toward Porcelain. Mark ramp |

Watermelon is never used for body text, labels or any text that carries meaning. It marks state, emphasis and the logo fill.

## UI theme tokens

These values match `Branding/tokens.json` and the reference page tokens.

| Token | Light | Dark |
|---|---|---|
| Background | `#FDFFFC` | `#262626` |
| Surface | `#F1F3F0` | `#333333` |
| Border | `#DFE0DE` | `#4B4B4B` |
| Ink | `#333333` | `#FDFFFC` |
| Ink muted | `#767976` | `#9A9B99` |
| Accent (fills and icons) | `#EF2D56` | `#EF2D56` |

Radii: 4px for small elements, 10px for medium surfaces.

## Typography

- Wordmark and headings: JetBrains Mono, weight 600, letter-spacing -0.01em.
- Body text: IBM Plex Sans, weights 400, 500 and 600, 16px, line height 1.6.
- Eyebrow labels: JetBrains Mono, 12px, uppercase, letter-spacing 0.08em, muted ink.

Both families are licensed under the SIL Open Font License. The font files used for outlining are in `fonts/` in the design workspace. Outlined artwork does not need the fonts installed.

## Logo

The logo has two parts: the lowercase wordmark `lucubra` in JetBrains Mono 600, and a Watermelon bookmark ribbon that hangs from the baseline beneath the middle letter. The text is outlined to paths in every master.

Masters:

- `logo-wordmark.svg`: Graphite ink on an opaque Porcelain ground.
- `logo-wordmark-transparent.svg`: Graphite ink, no ground. Use on light surfaces.
- `logo-wordmark-dark.svg`: Porcelain ink on an opaque Graphite ground.
- `logo-wordmark-dark-transparent.svg`: Porcelain ink, no ground. Use on dark surfaces.

### Icon

The app icon is a Graphite tile with a 22% corner radius. The mark sits at 70% of the tile width.

- `icon.svg`: the app tile, with the Mosaic ribbon mark (see below).
- `icon-mark.svg`: the Mosaic ribbon mark alone, transparent, on the 900 by 900 grid.
- `favicon.svg`: the app tile with a solid Watermelon ribbon, for tabs and small sizes.
- `icon-16.png` to `icon-1024.png` and `icon.ico` (16 to 256) are rasterised from `icon.svg`.

The Mosaic ribbon mark uses the 9 by 9 grid of 100px cells. The centre cell is empty. Cells are circles (62% of the cell) or rounded squares (66%, corner radius 12%). Every cell inside the ribbon silhouette is filled, which is a deliberate departure from the standard fill falloff. Without it the ribbon does not read as a shape at any size.

### Small sizes

The Mosaic ribbon reads as a ribbon at 64px and 32px. At 16px the cells are too small to hold the silhouette, so the tab icon uses the solid ribbon in `favicon.svg`. Do not use `icon.svg` at 16px.

### Clearspace and minimum sizes

- Clearspace: one cell-width (one ninth of the mark box, or one letter-width of the wordmark) on all sides.
- Mark alone: 32px minimum, using `icon-mark.svg`. Use `favicon.svg` below 32px.
- Wordmark: 120px wide minimum.

### Do not

- Do not redraw the mark by hand. Regenerate it from the generator.
- Do not set the wordmark in any face other than JetBrains Mono.
- Do not place the Watermelon ribbon on a Watermelon ground.
- Do not add gradients, glows, shadows or outlines to the mark.
- Do not ship grid lines as part of any asset.

## Banner and social preview

- `banner.png` (1200 by 300): Porcelain ground. Mark, wordmark and tagline. Use in the README.
- `banner-dark.png` (1200 by 300): Graphite ground, Porcelain ink.
- `social-preview.png` (1280 by 640): Graphite ground, Porcelain ink. Under 1 MB.

The tagline is "Local-first study engine for any AI assistant". It is set in IBM Plex Sans 400 and uses the ground ink, not Watermelon.

## Accessibility: contrast

Ratios are WCAG 2.1 relative luminance contrast, computed from the hex values above. AA requires 4.5:1 for normal text, 3:1 for large text (24px, or 18.66px bold) and 3:1 for non-text graphics.

| Foreground | Background | Ratio | Use | Result |
|---|---|---|---|---|
| Graphite `#333333` | Porcelain `#FDFFFC` | 12.57 | Body text, headings, wordmark | Pass AA (normal) |
| Porcelain `#FDFFFC` | Graphite `#333333` | 12.57 | Body text, headings, wordmark | Pass AA (normal) |
| Porcelain `#FDFFFC` | Graphite `#262626` | 15.05 | Dark UI body text | Pass AA (normal) |
| Ink muted `#767976` | Porcelain `#FDFFFC` | 4.38 | Secondary text, light UI | Fail AA (normal). Pass AA large only |
| Ink muted `#6B6D6B` | Porcelain `#FDFFFC` | 5.19 | Proposed replacement for light UI secondary text | Pass AA (normal) |
| Ink muted `#9A9B99` | Background `#262626` | 5.42 | Secondary text, dark UI | Pass AA (normal) |
| Watermelon `#EF2D56` | Porcelain `#FDFFFC` | 4.03 | Icons, fills, focus rings | Pass non-text 3:1. Not for text |
| Watermelon `#EF2D56` | Graphite `#333333` | 3.12 | Icons, fills, focus rings | Pass non-text 3:1. Not for text |
| Porcelain `#FDFFFC` | Watermelon `#EF2D56` | 4.03 | Large text on accent | Pass AA large only |
| Graphite `#333333` | Watermelon `#EF2D56` | 3.12 | Not for text | Fail AA normal text |
| Black `#000000` | Watermelon `#EF2D56` | 5.18 | Normal-size label on accent button | Pass AA (normal) |

Two pairs need action before the UI ships:

- Light-theme secondary text uses `#767976`, which fails AA for normal text at 4.38. Replace it with `#6B6D6B` (5.19) for body-size secondary text.
- Labels on Watermelon must use black `#000000` (5.18), not Graphite (3.12).

## Checklist for new assets

- Only the tokens above are used. One accent hue.
- Watermelon carries no normal-size text.
- Centre cell is empty in every Mosaic mark.
- No grid lines, gradients or glow in shipped artwork.
- Wordmark is outlined. No asset depends on an installed font.
