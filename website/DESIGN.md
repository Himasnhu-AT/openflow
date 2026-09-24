---
name: OpenFlow website
description: Private, local-first dictation for the Mac, told in the language of a mechanical keyboard.
colors:
  plate-cobalt: "#1d3fd6"
  plate-cobalt-deep: "#132a9e"
  cap-alpha: "#e8eaf0"
  cap-alpha-side: "#c3c8d4"
  cap-mod: "#262b36"
  cap-mod-side: "#12151c"
  cap-accent: "#d6f250"
  cap-accent-side: "#a6c21c"
  legend-ink: "#10131a"
  on-plate: "#f3f5ff"
  on-plate-soft: "#c9d1ff"
typography:
  display:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "clamp(2.75rem, 7.2vw, 6rem)"
    fontWeight: 800
    lineHeight: 0.95
    letterSpacing: "-0.03em"
    fontVariation: "'wdth' 112"
  headline:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "clamp(2rem, 4.2vw, 3.5rem)"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "-0.025em"
    fontVariation: "'wdth' 108"
  body:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "1.125rem"
    fontWeight: 400
    lineHeight: 1.55
  lede:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "clamp(1.15rem, 1.6vw, 1.35rem)"
    fontWeight: 400
    lineHeight: 1.5
  headline-small:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "clamp(1.6rem, 3vw, 2.4rem)"
    fontWeight: 800
    lineHeight: 1
  code:
    fontFamily: "ui-monospace, 'SF Mono', Menlo, monospace"
    fontSize: "0.95rem"
    lineHeight: 1.7
  legend:
    fontFamily: "Archivo, 'Helvetica Neue', Arial, sans-serif"
    fontSize: "0.8125rem"
    fontWeight: 600
    letterSpacing: "0.04em"
    fontVariation: "'wdth' 88"
rounded:
  kbd: "6px"
  cap-small: "12px"
  cap: "14px"
  screen: "18px"
  cap-giant: "22px"
  plate: "28px"
  chord: "0.14em"
spacing:
  u: "4px"
  gap: "12px"
  section: "clamp(96px, 14vw, 180px)"
components:
  button-return:
    backgroundColor: "{colors.cap-accent}"
    textColor: "{colors.legend-ink}"
    rounded: "{rounded.cap}"
    padding: "18px 28px 24px"
  button-mod:
    backgroundColor: "{colors.cap-mod}"
    textColor: "{colors.cap-alpha}"
    rounded: "{rounded.cap}"
    padding: "14px 20px 20px"
  key-alpha:
    backgroundColor: "{colors.cap-alpha}"
    textColor: "{colors.legend-ink}"
    rounded: "{rounded.cap}"
---

# Design System: OpenFlow website

## Overview

**Creative North Star: "The Colorway Sheet"**

OpenFlow is one chord, ⌘;. The site is built from the material of a mechanical keyboard: sculpted PBT keycaps with dye-sub legends, sitting on an anodized plate. A keyboard reads from above, like a colorway render sheet. Every interactive thing is a key. It has a top surface, a darker skirt below it, and a travel distance. Pressing a key sinks it. Information is printed the way keycaps print it: the main legend on top, a sub-legend on the front face.

The page is light-filled and saturated, because it's a keyboard on a desk in daylight rather than a dark dev tool. A cobalt plate carries most of the page. Light-grey alphas carry reading. Charcoal modifiers carry structure. One lime accent colour is reserved for the keys that matter: ⌘, ; and Return (Download).

**Key Characteristics:**
- Keys are 3D objects: a top face on a solid offset-shadow skirt, never flat cards.
- Legends and sub-legends: headline on the cap, detail printed on the front face.
- Cobalt plate as the page field. Alphas for reading surfaces. Lime only on the ⌘ ; Return family.
- One authored motion: key travel (press, sink, release). Nothing floats in on scroll.

## Colors

This is a committed colorway: a saturated cobalt plate owns 40–60% of the page, with neutral caps and one lime accent family.

### Primary
- **Anodized Cobalt Plate** (plate-cobalt): the page ground and section fields. It's the keyboard case the caps sit on.
- **Deep Cobalt** (plate-cobalt-deep): recessed wells between caps and the plate's inner shadow.

### Secondary
- **Lime Accent Cap** (cap-accent, skirt cap-accent-side): only on ⌘, ;, the Return-shaped Download key, and the active state of a key group.

### Neutral
- **PBT Alpha Grey** (cap-alpha, skirt cap-alpha-side): reading surfaces such as demo screens, notes and legend sheets.
- **Charcoal Modifier** (cap-mod, skirt cap-mod-side): nav keys, secondary buttons, and terminal and code surfaces.
- **Legend Ink** (legend-ink): text on alphas and on accent caps.
- **Plate White** and **Plate Soft** (on-plate, on-plate-soft): text directly on cobalt. Secondary text is tinted from the plate's hue, never grey.

**The Lime Budget Rule.** Lime appears only on ⌘, ; and Return, plus at most one active key per group. If a second, unrelated thing turns lime, remove it.

## Typography

**Display, Body and Legend Font:** Archivo (variable: wght and wdth), falling back to Helvetica Neue and Arial.
**Code Font:** the system monospace (ui-monospace, SF Mono, Menlo), used only for real commands and terminal output, never as decoration.

**Character:** a single engineered grotesk, stretched wide for the headlines that sit on the plate and narrowed for legends, the way keycap legends use one family at different widths.

### Hierarchy
- **Display** (800, clamp(2.75rem, 7.2vw, 6rem), 0.95, wdth 112): the single hero line.
- **Headline** (800, clamp(2rem, 4.2vw, 3.5rem), 1.0, wdth 108): section statements.
- **Body** (400, 1.125rem, 1.55): prose, capped at a 62ch measure.
- **Legend** (600, 0.8125rem, +0.04em, wdth 88): text printed on keys, as main legends and front sub-legends. Sentence case unless the real key is a symbol.

**The Legend Rule.** Text on a key is short and printed like a legend: a symbol or one to three words on top, and the explanation on the front face.

## Layout

- Sections are plates: full-bleed cobalt or alpha fields with generous vertical space (spacing.section).
- Content sits on a 12-column grid, max 1200px.
- Key grids use a 1u unit of 64–84px depending on the viewport (56px on phones). Legend-sheet keys come in three widths (2.3u, 2.9u, 3.6u) and grow to fill their row, so rows end flush like a real keyboard. Pipeline and requirement rows use an auto-fit grid with a 170px minimum.
- On phones the legend sheet goes 2-up, with the widest keys taking a full row. Keys are never squashed below their legend.

## Elevation & Depth

Depth is physical key height, not ambient glow. Each key is a top face with a solid, darker skirt drawn as an offset shadow (`--depth`: 4px tiny, 7px standard, 11px giant), plus a soft drop below it. Pressing moves the key down by its depth, onto its skirt, using transform only; layout never moves. A selected key in a group sits half-pressed (3px). The L-shaped Return key draws its skirt as a clipped layer, because clip-path would cut a shadow.

### Shadow Vocabulary
- **Cap rest** (`inset 0 0 0 1px rgba(0,0,0,.06), 0 var(--depth) 0 var(--skirt), 0 calc(var(--depth) + 6px) 16px var(--drop)`): every key at rest. `--drop` is cobalt-tinted on cobalt plates, ink-tinted on light plates.
- **Cap pressed** (`0 1px 0 var(--skirt), 0 3px 6px var(--drop)`, translateY(depth − 1px)): held or active keys.

## Shapes

Keys are rounded rectangles (12px small, 14px standard, 22px giant) with a soft top highlight, like a sculpted dish. The Download key is ISO-Return shaped (an L), made with clip-path. The plate has a 28px radius where it's framed.

## Components

### Keys (signature)
- **Alpha, Mod, Accent** variants share one structure: skirt, top face, legend and optional front sub-legend.
- **States:** rest; hover (top face lightens 3%); pressed or held (sinks by its depth onto the skirt); focus-visible (3px lime ring, offset 4px, on mods and alphas; ink ring on accent keys); hover pre-travel of 1px; disabled (no skirt, 50% legend).

### Buttons
- **Return (primary):** a lime ISO-Return key labelled "Download for Mac", with a "macOS 15+ · Apple Silicon" sub-legend.
- **Mod (secondary):** a charcoal key, e.g. "View source".

### Function-row keys (navigation)
Small charcoal keys with an F-number legend above the section name (F1 What it fixes … F4 Open source).

### Fact caps and rows
Facts are alpha keys with a bold legend and a front sub-legend, never bullet lists. A plain `ul` of facts is off-system.

### Plate wells
Screenshots sit in a recessed well cut into the plate: deep cobalt on cobalt plates, and #d9dce5 on light plates, with an inset shadow.

### Demo screen
A PBT-alpha surface framed as a Mac text field (Messages, Mail, Terminal). It shows live words arriving as you hold the keys, then the polished text replacing them. It is always labelled as a simulation.

## Do's and Don'ts

### Do:
- **Do** make anything clickable a key with a skirt and travel.
- **Do** let people press the real ⌘; on their keyboard to run the demo.
- **Do** tint secondary text on cobalt from the plate hue (on-plate-soft).

### Don't:
- **Don't** use flat, same-size icon cards. Group items as key clusters with legends instead.
- **Don't** use gradient text, glassmorphism or glow halos. Depth comes from key height.
- **Don't** put lime on anything outside the ⌘ ; Return family.
- **Don't** animate sections in on scroll. The only motion is key travel and the demo text.
