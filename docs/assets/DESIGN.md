# Lucubra brand sheet

Lucubra is a local-first study engine for students. The name comes from "lucubrate", to study by lamplight. This sheet records the logo, colour, type and sizing rules for the Lucubra release. It follows the uncoalesced Mosaic system (`MOSAIC_SYSTEM.md`) and the token file `Branding/tokens.json`, with the departures listed below.

## Colour

Lucubra uses two colours in brand artwork, plus one text colour.

| Token | Hex | Role |
|---|---|---|
| Graphite | `#333333` | The only ground for brand artwork |
| Porcelain | `#FDFFFC` | Text colour on Graphite (wordmark, tagline, body text) |
| Watermelon | `#EF2D56` | The only mark colour. Every square and circle in the mosaic, and the ribbon |

Brand artwork has no white or Porcelain grounds, and no light variants. The mark uses Watermelon at full strength. There are no shades, tints, darker variants or opacity changes. Watermelon is never used for text.

## UI theme tokens

These values match `Branding/tokens.json` and the reference page tokens. They apply to the application theme, not to brand artwork.

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

- `logo-wordmark.svg`: Porcelain text and Graphite ground. Default master.
- `logo-wordmark-dark-transparent.svg`: Porcelain text, no ground. Use on Graphite surfaces where the ground is supplied by the page.

There is no Graphite-ink master. Brand text is Porcelain on Graphite, so no light-surface variant is shipped.

### Icon

The app icon is a Graphite tile with a 22% corner radius. The mark sits at 70% of the tile width.

- `icon.svg`: the app tile, with the Mosaic ribbon mark.
- `icon-mark.svg`: the Mosaic ribbon mark alone, transparent, on the 900 by 900 grid.
- `favicon.svg`: the app tile with a solid Watermelon ribbon, for tabs and small sizes.
- `icon-16.png` to `icon-1024.png` and `icon.ico` (16 to 256) are rasterised from `icon.svg` at each size. The ICO frames are not resampled from the 256px image.

The Mosaic ribbon mark uses the 9 by 9 grid of 100px cells. Cells are circles (62% of the cell) or rounded squares (66%, corner radius 12%). Every cell inside the ribbon silhouette is filled, and the centre cell is filled too.

Lucubra departures from the Mosaic rules:

1. Filled centre cell. MOSAIC_SYSTEM leaves the centre cell empty. Lucubra fills it, so the mark has no hole at the centre. The centre cell is a fixed Watermelon circle, added after the random pass, so the other cells keep their shape and position for the seed.
2. Fill falloff removed inside the silhouette. Every cell inside the silhouette is kept (`--mask-floor 1.0`). The standard falloff leaves silhouettes too sparse to read.
3. Single colour. Mosaic product mode uses a shade, base and tint ramp. Lucubra uses Watermelon only, for every cell.

### Small sizes

The Mosaic ribbon reads as a ribbon at 64px and 32px. At 16px the cells are too small to hold the silhouette, so the tab icon uses the solid ribbon in `favicon.svg`. Do not use `icon.svg` at 16px.

### Clearspace and minimum sizes

- Clearspace: one cell-width (one ninth of the mark box, or one letter-width of the wordmark) on all sides.
- Mark alone: 32px minimum, using `icon-mark.svg`. Use `favicon.svg` below 32px.
- Wordmark: 120px wide minimum.

### Do not

- Do not redraw the mark by hand. Regenerate it from the generator.
- Do not set the wordmark in any face other than JetBrains Mono.
- Do not place the Watermelon ribbon or mosaic on a Watermelon ground.
- Do not use any ground other than Graphite for brand artwork.
- Do not add gradients, glows, shadows, outlines or opacity changes to the mark.
- Do not ship grid lines as part of any asset.

## Banner and social preview

- `banner.png` (1200 by 300): Graphite ground, Porcelain text. Mark, wordmark and tagline. Use in the README.
- `social-preview.png` (1280 by 640): Graphite ground, Porcelain text. Under 1 MB.

The tagline is "Local-first study engine for any AI assistant". It is set in IBM Plex Sans 400 and uses Porcelain, not Watermelon.

## Accessibility: contrast

Ratios are WCAG 2.1 relative luminance contrast, computed from the hex values above. AA requires 4.5:1 for normal text, 3:1 for large text (24px, or 18.66px bold) and 3:1 for non-text graphics.

| Foreground | Background | Ratio | Use | Result |
|---|---|---|---|---|
| Porcelain `#FDFFFC` | Graphite `#333333` | 12.57 | Wordmark, tagline, body text on brand artwork | Pass AA (normal) |
| Porcelain `#FDFFFC` | Graphite `#262626` | 15.05 | Dark UI body text | Pass AA (normal) |
| Watermelon `#EF2D56` | Graphite `#333333` | 3.12 | Mosaic mark, ribbon, focus rings (non-text fill) | Pass non-text 3:1. Not for text |
| Watermelon `#EF2D56` | Porcelain `#FDFFFC` | 4.03 | Icons and fills on UI surfaces | Pass non-text 3:1. Not for text |
| Porcelain `#FDFFFC` | Watermelon `#EF2D56` | 4.03 | Large text on accent | Pass AA large only |
| Black `#000000` | Watermelon `#EF2D56` | 5.18 | Normal-size label on accent button | Pass AA (normal) |
| Ink muted `#767976` | Porcelain `#FDFFFC` | 4.38 | Secondary text, light UI | Fail AA (normal). Pass AA large only |
| Ink muted `#6B6D6B` | Porcelain `#FDFFFC` | 5.19 | Replacement for light UI secondary text | Pass AA (normal) |
| Ink muted `#9A9B99` | Background `#262626` | 5.42 | Secondary text, dark UI | Pass AA (normal) |

Two pairs need action before the UI ships:

- Light-theme secondary text uses `#767976`, which fails AA for normal text at 4.38. Replace it with `#6B6D6B` (5.19) for body-size secondary text.
- Labels on Watermelon must use black `#000000` (5.18), not Graphite (3.12). Graphite on Watermelon is not permitted for text.

## Checklist for new assets

- Only Graphite, Porcelain (text only) and Watermelon appear in brand artwork.
- Graphite is the only ground.
- Watermelon is the only mark colour. No shades, tints, opacity changes or gradients.
- Watermelon carries no text.
- The centre cell is filled in every Mosaic mark (Lucubra departure).
- No grid lines, gradients or glow in shipped artwork.
- Wordmark is outlined. No asset depends on an installed font.
