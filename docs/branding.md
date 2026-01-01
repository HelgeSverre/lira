# Lira Brand Guidelines

## Brand Concept

**Lira** draws its identity from _Lyra_ — the constellation and the ancient lyre instrument. This connection represents:

- **Harmony** — Orchestrating concurrent fibers in perfect synchronization
- **Precision** — The exactness of tuned strings, like a well-typed program
- **Timelessness** — A guiding light, like the constellation that has oriented travelers for millennia

The brand should feel: **Modern, Technical, Warm, Precise**

---

## Logo & Symbol

### Primary Symbol: The Lyre Mark

A minimalist, geometric lyre that doubles as an abstract representation of converging fiber paths. The symbol consists of:

1. Two curved outer lines (the lyre frame / fiber paths)
2. Three vertical lines (strings / concurrent execution)
3. A base anchor point (the VM foundation)

```
    ╭─────╮
    │ │ │ │
    │ │ │ │
    │ │ │ │
    ╰──┬──╯
       │
```

**Usage:**

- Minimum size: 24px height
- Clear space: Equal to the height of the base anchor on all sides
- Never stretch, rotate, or modify the proportions

### Wordmark

"Lira" set in **Inter** (Bold, 700) with standard letter-spacing.

---

## Color Palette

### Primary Colors

| Name          | Hex       | RGB           | Usage                                         |
| ------------- | --------- | ------------- | --------------------------------------------- |
| **Lira Gold** | `#E8A924` | 232, 169, 36  | Primary brand color, CTAs, highlights         |
| **Night**     | `#1A1B26` | 26, 27, 38    | Primary background (dark mode), text          |
| **Snow**      | `#F5F5F7` | 245, 245, 247 | Primary background (light mode), text on dark |

### Secondary Colors

| Name          | Hex       | RGB           | Usage                                 |
| ------------- | --------- | ------------- | ------------------------------------- |
| **Starlight** | `#6B9AC4` | 107, 154, 196 | Links, secondary accents              |
| **Moss**      | `#7AA874` | 122, 168, 116 | Success states, positive indicators   |
| **Ember**     | `#E85A4F` | 232, 90, 79   | Errors, warnings, destructive actions |
| **Dusk**      | `#2D2F3D` | 45, 47, 61    | Card backgrounds, elevated surfaces   |
| **Mist**      | `#E8E8EC` | 232, 232, 236 | Borders, subtle backgrounds           |

### Gradients

**Hero Gradient** (for headers/hero sections):

```css
background: linear-gradient(135deg, #1a1b26 0%, #2d2f3d 100%);
```

**Accent Gradient** (for buttons/highlights):

```css
background: linear-gradient(135deg, #e8a924 0%, #f0c14b 100%);
```

---

## Typography

### Font Stack

**Primary Font:** Inter

- Headings: Inter Bold (700)
- Body: Inter Regular (400) / Medium (500)
- Fallback: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif

**Monospace Font:** JetBrains Mono

- Code blocks, inline code, terminal output
- Fallback: "Fira Code", "SF Mono", Consolas, monospace

### Type Scale

| Element | Size            | Weight | Line Height |
| ------- | --------------- | ------ | ----------- | --- |
| H1      | 48px / 3rem     | 700    | 1.2         |
| H2      | 36px / 2.25rem  | 700    | 1.25        |
| H3      | 24px / 1.5rem   | 600    | 1.3         |
| H4      | 20px / 1.25rem  | 600    | 1.4         | w   |
| Body    | 16px / 1rem     | 400    | 1.6         |
| Small   | 14px / 0.875rem | 400    | 1.5         |
| Code    | 14px / 0.875rem | 400    | 1.6         |

---

## Code Presentation

### Syntax Highlighting Theme (Lira Night)

Based on the Night color palette:

| Token      | Color     | Hex       |
| ---------- | --------- | --------- |
| Background | Night     | `#1A1B26` |
| Text       | Snow      | `#F5F5F7` |
| Comments   | —         | `#6B7280` |
| Keywords   | Lira Gold | `#E8A924` |
| Strings    | Moss      | `#7AA874` |
| Numbers    | Starlight | `#6B9AC4` |
| Functions  | Snow      | `#F5F5F7` |
| Types      | —         | `#C4A7E7` |
| Operators  | —         | `#89DDFF` |

---

## Visual Elements

### Constellation Pattern

A subtle background pattern of dots and thin lines, inspired by the Lyra constellation. Use sparingly:

- Hero backgrounds (very low opacity: 5-10%)
- Section dividers
- Empty states

### Iconography

Use simple, geometric line icons with:

- 1.5px stroke weight
- Rounded caps and joins
- 24px default size
- Snow color on dark backgrounds, Night on light

### Spacing

Use an 8px base grid:

- 8px, 16px, 24px, 32px, 48px, 64px, 96px, 128px

### Border Radius

- Small (buttons, inputs): 6px
- Medium (cards): 12px
- Large (modals, hero elements): 16px

### Shadows

**Elevation 1** (cards, dropdowns):

```css
box-shadow:
  0 4px 6px -1px rgba(0, 0, 0, 0.1),
  0 2px 4px -1px rgba(0, 0, 0, 0.06);
```

**Elevation 2** (modals, popovers):

```css
box-shadow:
  0 20px 25px -5px rgba(0, 0, 0, 0.1),
  0 10px 10px -5px rgba(0, 0, 0, 0.04);
```

---

## Voice & Tone

### Personality

- **Confident** but not arrogant
- **Technical** but accessible
- **Precise** — every word matters
- **Warm** — welcoming to newcomers

### Writing Style

- Use active voice
- Be concise — respect the reader's time
- Lead with benefits, follow with features
- Use code examples liberally

### Example Copy

**Good:**

> Lira gives you Go-like concurrency with fibers that feel natural to write and reason about.

**Avoid:**

> Lira is an innovative next-generation programming language that revolutionizes the way developers think about concurrent programming paradigms.

---

## Usage Examples

### Do

- Use Lira Gold sparingly for emphasis and CTAs
- Maintain high contrast ratios (WCAG AA minimum)
- Let code examples speak for the language
- Use the dark theme as the primary presentation

### Don't

- Use the logo on busy backgrounds without a container
- Pair Lira Gold with other warm colors
- Use more than 2-3 colors in a single composition
- Add drop shadows to the logo

---

## File Formats

### Logo Files

- `lira-logo.svg` — Primary vector logo
- `lira-logo-dark.svg` — For light backgrounds
- `lira-logo-light.svg` — For dark backgrounds
- `lira-icon.svg` — Symbol only, no wordmark
- `lira-favicon.ico` — 16x16, 32x32, 48x48

### Social Media

- `lira-og.png` — 1200x630 Open Graph image
- `lira-twitter.png` — 1200x600 Twitter card
- `lira-square.png` — 1080x1080 Instagram/social

---

_These guidelines ensure Lira maintains a consistent, professional identity across all touchpoints._
