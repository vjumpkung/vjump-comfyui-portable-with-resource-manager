# DESIGN.md — vjumpkung-rust-comfyui-desktop

This is the visual and interaction design system for the manager UI (`ui/`). It applies only to the manager webview; ComfyUI's own web UI is not restyled.

**Upstream.** This document is adapted from the vjump.dev design system:

| | |
|---|---|
| Source | `d:\vjump\vjump-dev-new-website\DESIGN.md` |
| Source fingerprint | sha256 `36a7e467c0c6ba12…` |
| Captured | 2026-10-15 |
| Token source | `src/app/globals.css` |
| Component config | `components.json`: `radix-vega`, `lucide` |

The tokens below are copied verbatim, so this repo is self-contained. When upstream changes, re-sync this file deliberately and note the new fingerprint.

Desktop exceptions are marked **[desktop]**.

---

## 1. Principles

1. **Soft slate canvas with three pastel accents.** Neutral surfaces are cool blue-grey slate. All color comes from yellow, cyan and pink. There are no saturated "primary blue" buttons.
2. **Border-first, flat surfaces.** Elements are separated by 1px `border-border` lines and `bg-card` surfaces. Shadows appear only on overlays (`shadow-2xl`).
3. **Hover = accent border or accent fill.** Interactive elements start neutral (`border-border bg-card text-muted-foreground`). On hover they take a brand border, or a brand fill with dark text.
4. **Monospace for metadata.** Sizes, hashes, versions, paths, speeds, ETAs, status chips, section eyebrows, keyboard hints and log output use `font-mono`. Chips and eyebrows are often uppercase with wide tracking.
5. **Tight, heavy headlines; small calm body.** Page titles use `font-extrabold tracking-tighter`. Body text uses `text-sm font-medium leading-relaxed`.
6. **[desktop] English and Thai copy for release 1.** Manager UI copy is user-selectable, with English as the default. Fonts MUST render Thai glyphs in UI copy, paths, model names and node names (§3).

**[desktop] Translation pattern.**

- Keep both locales in one `ui/src/shared/i18n/translations.json` catalog (`{ "en": {...}, "th": {...} }`), accessed through the `ui/src/shared/i18n/index.ts` module's `t()` / `useTranslation()`; the module keeps the current language in a zustand store. Load the catalog synchronously; existing bundler resolution supports JSON, so change TypeScript configuration only if typecheck requires it.
- Use flat semantic keys with dotted namespaces and camelCase segments. Translate complete messages with named `{placeholders}`, never concatenated translated fragments. Define `_one` / `_other` plural variants in both locales (Thai values may match). Prefer string interpolation; add a ReactNode translation component only when a sentence needs one.
- Store keys and data, not translated strings, in state and constant maps. Translate at render time or message-delivery time, using the current language after an asynchronous operation. `useTranslation()` makes mounted UI react to language changes.
- Translate all manager-owned visible copy, accessible names (`aria-label`, `title`, placeholders), toasts, dialogs and validation messages. Map status/enum labels by transport value and known setup step names by `step.step`; unknown values retain their existing raw or underscore-to-space fallback.
- Keep product and technical names untranslated. Display arbitrary backend/IPC errors, logs, external descriptions/release notes, paths, file names and identifiers as delivered; backend diagnostic text and logs remain English.
- The backend setting is authoritative. Cache it in localStorage (`vjumpkung-comfyui.language`) for first paint and update `<html lang>`; default to `en` without browser-language detection. Recovery/rebuild routes use only the cached language and make no settings calls.
- Show language options by endonym: **English** / **ไทย**. Put the selector at the top of the existing first setup stage (**Install root**), keeping six stages and no header language control. Apply the wizard choice locally at once, save the full settings, reconcile with the returned language and restore the previous language on failure. Settings keeps language as a draft until a successful **Save**. The app layer coordinates wizard preferences without feature-to-feature imports.
- Number and date formatting stays unchanged.

## 2. Color

### 2.1 Brand accents (fixed in both themes)
| Token | Hex | Role in this app |
|---|---|---|
| `brand-yellow` / `primary` | `#fff89c` | Active nav item, selected filter, primary action fill, "completed" chip |
| `brand-cyan` / `secondary` | `#9ffaff` | Card/row hover border, table header, input focus, progress fill, "running/downloading" chip |
| `brand-pink` / `accent` | `#ff9fed` | Active nav underline, link underline, `::selection`, light-mode `ring`, "paused/attention" chip |

**Rule: text on a brand fill is always `#2a313a`** (`text-[#2a313a]` or `*-foreground`) in both themes.

### 2.2 Neutral / semantic tokens
| Token | Light | Dark | Usage |
|---|---|---|---|
| `background` | `#e0e5ec` | `#2a313a` | App canvas, title/top bar (`/80` + blur) |
| `foreground` | `#2a313a` | `#e0e5ec` | Primary text |
| `card` / `popover` | `#edf1f5` | `#343d48` | Panels, sidebar, inputs, dialogs, tables |
| `muted` | `#d2d8e0` | `#3b4652` | Hover rows, kbd, progress track, zebra stripes |
| `muted-foreground` | `#56606c` | `#b8c0c9` | Secondary text, meta, icons |
| `border` / `input` | `#aab3be` | `#596674` | All 1px rules and outlines |
| `ring` | `#ff9fed` | `#9ffaff` | Focus rings |
| `destructive` | `oklch(0.577 0.245 27.325)` | `oklch(0.704 0.191 22.216)` | **[desktop]** Used for delete/uninstall confirmations and the `failed` state. Upstream does not use it in custom UI. |

- Light and dark swap `#2a313a` and `#e0e5ec`.
- **Code, log and terminal surfaces use dark slate `#2a313a` in both themes.**
- Chart tokens `chart-1…5` map to yellow, cyan, pink, muted-fg and fg.

### 2.3 Theming
- `next-themes` (framework-agnostic React) with `attribute="class"`, `defaultTheme="system"`, `enableSystem` and `disableTransitionOnChange`. Its injected script and style MUST pass the production CSP. If they don't, replace it with a minimal provider that has the same semantics.
- Dark variant: `@custom-variant dark (&:is(.dark *))`.
- The theme toggle is a ghost icon button (Sun/Moon) in the top bar and also lives in Settings.

## 3. Typography
| Role | Font | Notes |
|---|---|---|
| Sans | **Inter** → `--font-sans`, `--font-heading` | Bundled locally (`@fontsource-variable/inter`) **[desktop: no runtime Google Fonts; CSP and offline]** |
| Mono | **Geist Mono** → `--font-mono` | Bundled locally |
| Thai fallback | **Noto Sans Thai** / **Noto Sans Thai Looped** for mono contexts | **[desktop]** Added to both stacks, because Inter and Geist Mono have no Thai glyphs. Rendering is tested in WebView2 and WebKitGTK. |

Type scale:
| Element | Classes |
|---|---|
| Page title | `text-2xl sm:text-3xl font-extrabold tracking-tighter` |
| Panel/section title | `text-lg font-bold tracking-tight` |
| Section eyebrow | `font-mono text-xs font-semibold uppercase tracking-wider text-muted-foreground` |
| Body | `text-sm font-medium leading-relaxed` |
| Meta (size, version, ETA) | `font-mono text-xs text-muted-foreground` |
| Chip | `font-mono text-[9px] uppercase tracking-widest` |

When the interface language is Thai, 9px monospace chips render at 11px without wide tracking because Thai vowel and tone marks are illegible at 9px.

Long paths, model names and URLs wrap with `break-words [overflow-wrap:anywhere]`, or truncate with a tooltip in tables.

## 4. Shape, spacing, layout
- **Radius base** `--radius: 0.625rem`. Scale: `sm ×0.6`, `md ×0.8`, `lg ×1`, `xl ×1.4`, `2xl ×1.8`, `3xl ×2.2`, `4xl ×2.6`.
  - `rounded-md`: buttons, inputs, chips, nav items
  - `rounded-lg`: tables, dialogs, log panels
  - `rounded-xl`: cards
  - `rounded-full`: avatars, circular icon buttons
- **[desktop] App shell** (replaces the website's centred `max-w-5xl` container):
  - left sidebar: `w-56`, `bg-card border-r`, collapsible to icons at `< md`;
  - top bar: `h-14 bg-background/80 backdrop-blur-md border-b`, holding the ComfyUI status chip, Start/Stop button and theme toggle;
  - content: `p-6`, sections stacked with `flex flex-col gap-6`;
  - minimum window width 960px.
- Sidebar items, in order: **Overview**, **ComfyUI**, **Downloads**, **Models**, **Custom Nodes**, **Environment**, **Settings**.
  - Active item: `bg-brand-yellow text-[#2a313a] font-semibold`.
  - Inactive item: muted, and gets a pink left bar on hover.

## 5. Components and patterns
- **Buttons.** shadcn `Button`, style **`radix-vega`**, with variants `default | outline | secondary | ghost | destructive | link`. Primary actions fill yellow, outline buttons hover cyan or pink, and destructive actions use the `destructive` variant. Press feedback is `translate-y-px`.
- **Cards and panels.** `rounded-xl border border-border bg-card p-5 transition-all duration-300` with `hover:border-brand-cyan`, or pink for a neighbouring card. Alternating accents between neighbours is deliberate.
- **Tables** (models, nodes, downloads, packages):
  - wrapper: `overflow-x-auto rounded-lg border bg-card`;
  - header: cyan `bg-brand-cyan text-[#2a313a]`;
  - zebra rows `even:bg-muted/45`, hover rows `hover:bg-muted`;
  - Active row: `bg-muted` with a 3px accent inset bar (`shadow-[inset_3px_0_0_var(--accent)]`); checked rows `bg-muted/60`. Never put a brand fill behind table text (contrast).
  - columns for size, hash and version use mono;
  - lists over 200 rows are virtualized.
- **Status chips [desktop].** These use the chip style. Each one MUST include a text label and a Lucide icon, never color alone.

  | State | Style | Icon |
  |---|---|---|
  | queued / stopped | `bg-muted text-muted-foreground border-border` | `Clock` / `Square` |
  | resolving / starting / verifying | cyan outline | `Loader2` (spin, `motion-safe`) |
  | downloading / running | cyan fill | `ArrowDownToLine` / `Play` |
  | paused / repair required / deps-failed | pink fill | `Pause` / `Wrench` / `TriangleAlert` |
  | completed / verified / active env | yellow fill | `Check` / `ShieldCheck` |
  | unverified | yellow outline | `ShieldQuestion` |
  | failed / crashed | `destructive` fill, light text | `CircleX` |

- **Progress bar [desktop].** `h-1.5 rounded-full bg-muted` track with a cyan fill, or pink when paused. Beside it, mono `bytes / total · speed · ETA`. For hf-CLI jobs, speed reads as an estimate (FR-DL-7).
- **Inputs.** `rounded-md border bg-card text-xs`, with an icon at left `pl-9`. Focus: `border-brand-cyan ring-2 ring-brand-cyan/35`. The clear button is a small round `X`.
- **Dialogs.** Overlay `bg-[#2a313a]/80 backdrop-blur-xs`; panel `rounded-lg bg-popover shadow-2xl`. Destructive confirmations state what will happen, e.g. "Move 3 files (12.4 GB) to Recycle Bin".
- **Log / terminal [desktop].** xterm on `#2a313a` in both themes, with a cyan border, `rounded-lg`, Geist Mono and a hover-revealed copy button. Theme mapping: cursor and selection pink, blue ANSI → cyan, yellow ANSI → brand yellow.
- **Toasts.** Sonner styled as `bg-popover border` with no shadow beyond overlays. Accent left bar: cyan for info, yellow for success, pink for warning, destructive for error.
- **Empty states.** `rounded-xl border border-dashed p-12 text-center text-sm text-muted-foreground`, plus one primary action, e.g. "Add download".
- **Links.** `font-semibold underline decoration-brand-pink decoration-2 underline-offset-2`. External links open in the system browser and show an `ExternalLink` icon.

## 6. Motion
- Default `transition-colors`; cards use `transition-all duration-300`.
- Micro-motion: arrow nudge on hover, button press, a 2 s cyan check after copying.
- All animation is `motion-safe:`. Theme switching is instant.
- Upstream known gaps (`animate-fade-in`, `scrollbar-thin`) are **not** used. Use `tw-animate-css` `animate-in fade-in` instead, and style scrollbars in CSS if needed.

## 7. Accessibility
- Every interactive element has `focus-visible:ring-2 ring-ring`. Full keyboard navigation covers the sidebar, tables (arrow keys), dialogs (focus trap, Esc) and the download queue.
- Icon-only buttons have `aria-label`s translated into the selected interface language. Dialogs use `role="dialog" aria-modal="true"`.
- Text on brand fills is always `#2a313a`. WCAG AA contrast is verified for all chip and button states in both themes.
- Status is never conveyed by color alone (§5).
- Layout survives 200 % zoom and long Unicode paths.

## 8. Iconography
- **lucide-react** for all UI icons, usually `h-3.5`–`h-4`. This replaces Remixicon from the reference frontend.
- App icon and logo: TBD (upstream `vjumpkung-landscape.svg` may be reused with permission of the owner).

## 9. Not adopted from upstream
- MDX/article typography, blog cards/bento, image lightbox, GitHub gist embeds, and `search.json` search.
- A `⌘K` command palette is not part of release 1.
- Thai-first copy; the desktop UI offers English and Thai, defaulting to English (see §1.6).
