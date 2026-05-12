# Backr — technical UI aesthetic

Design reference for layout, typography, color tokens, and interaction patterns. Use this when extending screens, marketing screenshots, or documentation diagrams. Product-specific strings live in the Svelte app (`src/`); **tokens are canonical in `src/app.css`.**

---

## Summary

Dark, **monospace-first** desktop UI with low-glare surfaces and a **cool blue** primary accent suitable for technical tooling (terminal-adjacent, monitoring, infra).

Goals:

- High signal-to-noise: dense labels, tabular numbers, explicit status text.
- Predictable structure: sidebar navigation, bordered panels, restrained motion.
- Accessibility: sufficient contrast on body and small uppercase labels; visible focus rings.

Non-goals: decorative gradients, mascot-heavy marketing, playful rounded “consumer” chrome.

---

## Typography

| Role | Specification |
|------|----------------|
| **Primary** | **IBM Plex Mono** (400–700). |
| **Fallbacks** | `ui-monospace`, `monospace`. |
| **Usage** | Headings, body, controls—single stack reads as instrumentation, not editorial. |

**Micro-labels**

- Section labels: uppercase, ~10–11px, letter-spacing ~0.12–0.16em.
- Body default ~15px; scale headings with `clamp` where needed.

Load IBM Plex Mono via Google Fonts or bundle locally; match weights used in `index.html`.

---

## Color tokens (CSS variables)

Defined in `src/app.css`. Prefer variables over raw hex in components.

### Surfaces & text

| Token | Hex | Role |
|-------|-----|------|
| `--bg` | `#0a0c10` | Default page background |
| `--bg2` | `#0d1016` | Sidebar / secondary regions |
| `--bg3` | `#12151c` | Cards / elevated panels |
| `--bg4` | `#161b26` | Inputs / inset wells |
| `--border` | `#1e2433` | Default borders |
| `--border2` | `#2a3347` | Strong separators |
| `--border-glow` | `#34405a` | Hover emphasis |
| `--text` | `#c8cdd8` | Primary text |
| `--muted` | `#6b7289` | Secondary labels |
| `--muted2` | `#8b95a8` | Tertiary / helper text |

### Actions & focus

| Token | Hex | Role |
|-------|-----|------|
| `--accent` | `#3d9cf0` | Primary actions, links, focus ring baseline |
| `--accent-hover` | `#5aaefb` | Hover |
| `--accent-pressed` | `#2e8ad9` | Active / pressed |

### Semantic states

| Token | Hex | Role |
|-------|-----|------|
| `--danger` | `#f07178` | Errors, destructive actions, critical warnings |
| `--success` | `#3dd68c` | Success, healthy / completed |
| `--info` | `#5a9fd4` | Informational highlights |
| `--warn` | `#e8b339` | Caution, attention-required (non-destructive) |
| `--deep` | `#b8a3e0` | Secondary categorical accent (tags, charts) |

Pair **`#3d9cf0`** on **`#0a0c10`** for hero / logo lockups; verify WCAG contrast when inventing new pairings.

---

## Layout & shape

- **Radius**: `5px` controls and rows; `8px` large cards / modals.
- **Panels**: Rectangles with subtle **inset top highlight** (`panel-plate` pattern) for depth without blur stacks.
- **App shell**: Left navigation rail + main content; optional secondary column for logs or detail on wide layouts.
- **Dividers**: Thin horizontal rules and left borders for nested sections.

---

## Motion

- Transitions ~150ms for hover/focus where helpful.
- Respect **`prefers-reduced-motion`** (global reductions belong in `src/app.css`).

---

## Iconography

- Prefer **simple geometric / outline icons** (stroke icons, small squares) with semantic tint (`--accent`, `--muted`, state colors).
- Avoid illustrative characters unless explicitly requested.

---

## Copy tone (UI strings)

- **Technical and imperative**: state what the system does, what failed, and the next action.
- Prefer **nouns from the domain** (snapshot, rsync, SSH, schedule, restore path) over metaphor.
- **Errors**: include cause or remediation when the backend provides it; avoid vague reassurance.

---

## Repository mapping

| Concern | Location |
|---------|-----------|
| Tokens & global base styles | `src/app.css` |
| Shell layout & routed views | `src/App.svelte`, `src/components/` |
| Dev UI mock (`VITE_BACKR_MOCK`) | `src/lib/useDevMock.ts`, `src/lib/devMock/` |

If this document disagrees with shipped CSS or components, **update this file** to match the implementation.

### UI mock mode (development only)

`npm run dev:mock` or `npm run tauri:dev:mock` sets `VITE_BACKR_MOCK=1`. Alternatively in dev tools: `localStorage.setItem('backr-dev-mock','1')` then reload. Production builds never honor mock flags.
