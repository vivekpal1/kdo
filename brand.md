# Brand — kdo

_Status: applied (manual, from references — `brand-design` skill not run)_

The kdo brand is documented here so future sessions inherit the same direction
without re-prompting the user. References that informed every decision below:

- [liquidglassdesign.com](https://www.liquidglassdesign.com/) — frosted-glass pills,
  organic photography backgrounds, "Elegant" minimalism
- [wonder.app](https://wonder.app) — generous whitespace, transparent floating
  header, dark footer with floating "marble" project icons
- Smith & Diction — _Branding Superhuman (and Grammarly and Coda)_ — soft modern
  type, restraint, one moment of color rather than a brand-color shower

## Voice

- Concise. Active voice. No marketing fluff.
- Honest about alpha status; names competitors by name.
- "Workspace manager for the agent era." — short, direct, no slogans.
- Write like a senior engineer who built the thing, not a copywriter.

## Color palette

Defined as OKLCH tokens in `web/site/src/styles/global.css` under `@theme`:

| Token            | Value                              | Use                                         |
|------------------|------------------------------------|---------------------------------------------|
| `--color-canvas` | `oklch(98.4% 0.003 240)`           | Page background — cool off-white            |
| `--color-canvas-2` | `oklch(96.5% 0.005 240)`         | Subtle surface (terminal body, alt rows)    |
| `--color-fog`    | `oklch(92% 0.006 240)`             | Inactive controls, dot separators           |
| `--color-line`   | `oklch(88% 0.006 240)`             | Hairline borders, grid background           |
| `--color-ink`    | `oklch(18% 0.01 240)`              | Primary text, primary CTA bg                |
| `--color-ink-2`  | `oklch(35% 0.012 240)`             | Body text                                   |
| `--color-ink-3`  | `oklch(55% 0.012 240)`             | Tertiary text, hints, decoration            |
| `--color-tide`   | `oklch(56% 0.105 200)`             | Single accent — deep liquid teal            |
| `--color-tide-soft` | `oklch(86% 0.06 200)`           | Selection, soft tide highlights             |
| `--color-deep`   | `oklch(14% 0.02 250)`              | Footer base — deep navy-black               |
| `--color-deep-2` | `oklch(20% 0.025 250)`             | Footer alt surface                          |

Rules:
- One accent color (`--color-tide`). No secondary brand color anywhere.
- Grays are cool-tinted (hue 240–250). Never mix in warm grays.
- Footer is deep navy-black (not pure black) so glass marbles read clean against it.

## Typography

- **Sans (UI):** `Inter Variable` via `@fontsource-variable/inter`. Weight range 400–600 only.
  Default weight 400; medium for buttons/headings (500); semibold for display (600).
- **Mono (code, numbers):** `Geist Mono` via `@fontsource/geist-mono`. Weights 400/500.
- **Display:** Inter at `text-[88px]` to `text-[180px]`, `font-semibold`,
  `tracking-[-0.05em]`, `leading-none`. The wordmark "kdo" is the only place this scale appears.
- **Body:** `text-base` to `text-xl`, `leading-snug` for hero copy, `leading-relaxed` for paragraphs.
- **Eyebrows / labels:** `font-mono text-xs uppercase tracking-[0.18em]` in `--color-tide`.
  This is the "section signal" — used to mark the start of every major section.
- **Code in prose:** inline `<code>` gets `bg-canvas-2` + `font-mono text-sm`.

## Visual language

The hero is anchored by a **liquid glass marble** — a translucent sphere with:

- Inner highlights from a 35%/28% radial gradient
- Tide-tinted bottom-right radial for color depth
- Inset shadows top + bottom for the glass thickness
- Two outer drop shadows (close + far) for floating
- A bright pinpoint highlight via `::after`

Rendered purely in CSS so the marble works at any size, scales sharply, and respects
`prefers-reduced-motion`. The `.marble` utility in `global.css` is the source of truth.

Marbles also live in the **footer**, one per discovered project — each tinted by language
hue. This is the "Wonder app marble shelf" but reframed as kdo's actual product
metaphor: every workspace project becomes a marble.

## Surfaces

- **`.glass`** — light glass primitive: `backdrop-filter: blur(20px) saturate(180%)`,
  white at 55%, two inset highlights for the bevel. Used for the header, feature cards,
  config card, comparison table, install tabs.
- **`.glass-dark`** — dark variant for use over the footer if a glass element is needed.
- **`.grid-canvas`** — barely-visible 64×64 px grid with a radial mask so it fades at edges.
  Pure visual rhythm; no semantic meaning.
- **`.crosshair`** — single-pixel `+` markers placed sparingly. Reads as "technical drawing"
  without crowding.

## Motion

All durations from `frontend-design-guidelines/animation.md`:

| Use                       | Duration | Easing                       |
|---------------------------|----------|------------------------------|
| Hover color/transform     | 100 ms   | `ease-out`                   |
| Press feedback            | 100 ms   | `ease-out`                   |
| Section fade-up on load   | 600 ms   | `var(--ease-out-glass)`      |
| Marble float              | 9 s / 13 s | `var(--ease-out-glass)`    |

`var(--ease-out-glass) = cubic-bezier(0.16, 1, 0.3, 1)` — overshoots slightly, then settles.
Reads as "physical" without being bouncy.

All non-essential motion is gated behind `@media (prefers-reduced-motion: no-preference)`.

## Hard rules

- Use design tokens. No inline hex, no `p-[13px]`. The only inline styles allowed are
  hue-based gradients on the per-language footer marbles (`hsl(${hue} ...)`).
- Real `<button>` / `<a>`. Never `<div onClick>`.
- Every interactive element has a visible `outline-tide` focus ring.
- Hit targets ≥ 40×40 px on touch.
- Body text passes WCAG AA against canvas (4.5:1).

## Out of scope (intentionally not added)

- Dark mode. The hero is unmistakably bright; a dark variant would dilute the brand.
  Footer already provides the dark moment.
- shadcn/ui. Astro is server-rendered HTML; pulling in React just for primitives is
  off-character for a tool that ships as one binary.
- Hero illustrations. The wordmark + marble + grid is the entire visual system.
- Icon library. Lucide is overkill — we use ~3 inline SVGs total.

## Re-running brand-design

If the user later runs `/brand-design`, that skill will detect this file's
`Status: applied` line and ask before overwriting. Treat that as an intentional
re-design, not an error.
