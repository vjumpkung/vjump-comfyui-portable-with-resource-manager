---
name: vjump.dev
description: Cool grey paper, dark ink and three highlighter pens. A reading-first system for mixed Thai and Latin content, in light and dark.
colors:
    # Unsuffixed keys are the light theme. A `-dark` key is not a separate variable:
    # it is the value of the same variable inside `.dark` (card-dark sets --card).
    background: '#e0e5ec'
    foreground: '#2a313a'
    card: '#edf1f5'
    card-foreground: '#2a313a'
    popover: '#f4f6f9'
    popover-foreground: '#2a313a'
    primary: '#fff89c'
    primary-foreground: '#2a313a'
    secondary: '#9ffaff'
    secondary-foreground: '#2a313a'
    accent: '#ff9fed'
    accent-foreground: '#2a313a'
    muted: '#d2d8e0'
    muted-foreground: '#4b5562'
    destructive: 'oklch(0.577 0.245 27.325)'
    border: '#b6bfc9'
    input: '#6f7a87'
    ring: '#a3237e'
    ink: '#2a313a'
    pink-ink: '#a3237e'
    brand-yellow: '#fff89c'
    brand-cyan: '#9ffaff'
    brand-pink: '#ff9fed'
    marker-yellow: '#fff89c'
    marker-cyan: '#9ffaff'
    marker-pink: '#ff9fed'
    note-bg: 'color-mix(in srgb, #ff9fed 22%, #edf1f5)'
    code-bg: '#2a313a'
    code-fg: '#e0e5ec'
    background-dark: '#2a313a'
    foreground-dark: '#e0e5ec'
    card-dark: '#343d48'
    card-foreground-dark: '#e0e5ec'
    popover-dark: '#343d48'
    popover-foreground-dark: '#e0e5ec'
    muted-dark: '#3b4652'
    muted-foreground-dark: '#b8c0c9'
    destructive-dark: 'oklch(0.704 0.191 22.216)'
    border-dark: '#4a5563'
    input-dark: '#808d9c'
    ring-dark: '#ff9fed'
    pink-ink-dark: '#ff9fed'
    note-bg-dark: 'color-mix(in srgb, #ff9fed 12%, #2a313a)'
    code-bg-dark: '#1f252c'
typography:
    display:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: 'clamp(2.25rem, 1.4rem + 3.4vw, 3.75rem)'
        fontWeight: 700
        lineHeight: 1.2
        letterSpacing: '-0.02em'
    headline:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: 'clamp(1.875rem, 1.4rem + 1.9vw, 2.625rem)'
        fontWeight: 700
        lineHeight: 1.25
        letterSpacing: '-0.02em'
    section-title:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.5rem'
        fontWeight: 700
        lineHeight: 1.35
        letterSpacing: '-0.01em'
    feature-title:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.625rem'
        fontWeight: 700
        lineHeight: 1.3
    list-title:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.25rem'
        fontWeight: 700
        lineHeight: 1.4
    row-title:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1rem'
        fontWeight: 600
        lineHeight: 1.45
    prose-h1:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.75rem'
        fontWeight: 700
        lineHeight: 1.3
        letterSpacing: '-0.01em'
    prose-h2:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.5rem'
        fontWeight: 700
        lineHeight: 1.4
        letterSpacing: '-0.01em'
    prose-h3:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.1875rem'
        fontWeight: 600
        lineHeight: 1.45
    lede:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1.125rem'
        fontWeight: 400
        lineHeight: 1.5556
    body-prose:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1rem'
        fontWeight: 400
        lineHeight: 1.85
    body-list:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '1rem'
        fontWeight: 400
        lineHeight: 1.8
    body-ui:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '0.9375rem'
        fontWeight: 400
        lineHeight: 1.65
    nav:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '0.9375rem'
        fontWeight: 500
        lineHeight: 1.5
    control:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '0.875rem'
        fontWeight: 500
        lineHeight: 1.4286
    label:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '0.8125rem'
        fontWeight: 500
        lineHeight: 1.5
    tag:
        fontFamily: 'Plus Jakarta Sans, Kanit, ui-sans-serif, system-ui, sans-serif'
        fontSize: '0.75rem'
        fontWeight: 600
        lineHeight: 1.3333
        letterSpacing: '0.05em'
    code-block:
        fontFamily: 'Geist Mono, Kanit, ui-monospace, monospace'
        fontSize: '0.8125rem'
        fontWeight: 400
        lineHeight: 1.7
    code-inline:
        fontFamily: 'Geist Mono, Kanit, ui-monospace, monospace'
        fontSize: '0.875em'
        fontWeight: 500
rounded:
    xs: '0.25rem'
    md: '0.5rem'
    lg: '0.625rem'
    xl: '0.875rem'
    full: '9999px'
spacing:
    gutter: '20px'
    gutter-wide: '32px'
    page-y: '40px'
    page-y-wide: '56px'
    section-gap: '80px'
    section-gap-wide: '96px'
    block-gap: '40px'
    group-gap: '24px'
    card: '24px'
    card-compact: '20px'
    target: '40px'
    target-large: '44px'
components:
    nav-link:
        textColor: '{colors.muted-foreground}'
        typography: '{typography.nav}'
        height: '40px'
    nav-link-hover:
        textColor: '{colors.foreground}'
    nav-link-active:
        textColor: '{colors.foreground}'
    nav-tab-active:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
        typography: '{typography.control}'
        rounded: '{rounded.full}'
        padding: '0 16px'
        height: '40px'
    nav-tab-hover:
        backgroundColor: '{colors.muted}'
        textColor: '{colors.foreground}'
    pill-control:
        backgroundColor: '{colors.card}'
        textColor: '{colors.foreground}'
        typography: '{typography.control}'
        rounded: '{rounded.full}'
        padding: '0 16px'
        height: '40px'
    pill-control-hover:
        backgroundColor: '{colors.muted}'
    filter-chip-active:
        backgroundColor: '{colors.foreground}'
        textColor: '{colors.background}'
        typography: '{typography.control}'
        rounded: '{rounded.full}'
        padding: '0 16px'
        height: '40px'
    icon-control:
        textColor: '{colors.muted-foreground}'
        rounded: '{rounded.full}'
        size: '40px'
    icon-control-hover:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
    scroll-to-top:
        backgroundColor: '{colors.primary}'
        textColor: '{colors.primary-foreground}'
        rounded: '{rounded.full}'
        size: '44px'
    category-tag-yellow:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
        typography: '{typography.tag}'
        rounded: '{rounded.full}'
        padding: '2px 10px'
    category-tag-cyan:
        backgroundColor: '{colors.brand-cyan}'
        textColor: '{colors.ink}'
        typography: '{typography.tag}'
        rounded: '{rounded.full}'
        padding: '2px 10px'
    category-tag-pink:
        backgroundColor: '{colors.brand-pink}'
        textColor: '{colors.ink}'
        typography: '{typography.tag}'
        rounded: '{rounded.full}'
        padding: '2px 10px'
    input-search:
        backgroundColor: '{colors.card}'
        textColor: '{colors.foreground}'
        rounded: '{rounded.lg}'
        padding: '0 40px'
        height: '44px'
    panel-card:
        backgroundColor: '{colors.card}'
        textColor: '{colors.card-foreground}'
        rounded: '{rounded.xl}'
        padding: '24px'
    link-card:
        backgroundColor: '{colors.card}'
        textColor: '{colors.card-foreground}'
        rounded: '{rounded.xl}'
        padding: '20px'
    contact-row:
        backgroundColor: '{colors.background}'
        textColor: '{colors.foreground}'
        typography: '{typography.control}'
        rounded: '{rounded.lg}'
        padding: '0 16px'
        height: '44px'
    contact-row-hover:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
    callout:
        backgroundColor: '{colors.note-bg}'
        textColor: '{colors.foreground}'
        typography: '{typography.body-list}'
        rounded: '{rounded.xl}'
        padding: '16px 20px'
    code-block:
        backgroundColor: '{colors.code-bg}'
        textColor: '{colors.code-fg}'
        typography: '{typography.code-block}'
        rounded: '{rounded.xl}'
        padding: '16px 48px 16px 16px'
    code-inline:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
        typography: '{typography.code-inline}'
        rounded: '{rounded.md}'
        padding: '2px 6px'
    code-inline-dark:
        backgroundColor: '{colors.muted-dark}'
        textColor: '{colors.brand-yellow}'
    copy-button:
        textColor: '{colors.code-fg}'
        rounded: '{rounded.md}'
        size: '32px'
    copy-button-hover:
        backgroundColor: '{colors.brand-yellow}'
        textColor: '{colors.ink}'
    table-header:
        backgroundColor: '{colors.brand-cyan}'
        textColor: '{colors.ink}'
        padding: '12px 16px'
    table-cell:
        textColor: '{colors.foreground}'
        padding: '10px 16px'
    search-dialog:
        backgroundColor: '{colors.popover}'
        textColor: '{colors.popover-foreground}'
        rounded: '{rounded.xl}'
        width: '36rem'
    search-result-selected:
        backgroundColor: '{colors.muted}'
        textColor: '{colors.foreground}'
        rounded: '{rounded.md}'
        padding: '12px'
    lightbox-close:
        backgroundColor: '{colors.brand-pink}'
        textColor: '{colors.ink}'
        rounded: '{rounded.full}'
        size: '40px'
    lightbox-close-hover:
        backgroundColor: '{colors.brand-yellow}'
---

# Design System: vjump.dev

## Overview

**Creative North Star: "The Highlighter Notebook"**

Every page reads like a well-kept notebook: cool grey paper, graphite ink, and three highlighter pens used only where attention should land. The interface steps back so long technical articles stay comfortable. Density is relaxed, with wide gaps between sections, a narrow reading column and generous line height, and Thai and Latin text sit together on one line without looking like two systems.

Colour is scarce and physical. The pastels behave like real highlighter: a swipe under a phrase, a filled tag, a table header. They are never body text, because pale on pale cannot be read. Depth comes from stacked sheets of slightly different tone, not from shadow. The dark theme keeps the same pens on a slate desk, and since a swipe behind light text would wash it out, the swipe becomes a coloured underline there.

Motion stays out of the way. One signature gesture exists, the marker drawing itself across the hero phrase once on load. Everything else is a short colour change.

**Key Characteristics:**

- One neutral ramp plus exactly three accents, all pastel, always paired with ink text.
- A marker highlight as the single signature element.
- One sans family across headings and body, with hierarchy from size, weight and tracking.
- Flat tonal surfaces, with hairlines separating content and shadow reserved mostly for floating layers.
- Pill-shaped compact controls and labels against softly rounded containers.
- A reading column of 44rem at 1.85 line height.
- Full light and dark themes that swap the same two values for page and text.

### Adopting This System

1. **Colours.** Declare each unsuffixed colour key as a CSS variable on `:root` (`card` becomes `--card`). Declare each `-dark` key as the same variable inside `.dark` (`card-dark` sets `--card`). Keys without a `-dark` sibling, such as `ink`, stay constant across themes on purpose. The token names follow shadcn/ui conventions, so they drop into a shadcn project unchanged.
2. **Theme switching.** The site toggles a `.dark` class on the root element and defaults to the system preference.
3. **Fonts.** Load Plus Jakarta Sans and Geist Mono with their Latin subsets, and Kanit with its Thai subset limited to `U+02D7, U+0303, U+0331, U+0E01-0E5B, U+200C-200D, U+25CC` so it never replaces Latin glyphs. Listing Jakarta first lets Latin use Jakarta while Thai falls through to Kanit. If the project has no Thai content, remove Kanit from both stacks.
4. **Tailwind v4 wiring**, if the target uses it:

```css
@custom-variant dark (&:is(.dark *));

@theme inline {
    /* One line per colour key, excluding -dark keys */
    --color-background: var(--background);
    --color-foreground: var(--foreground);

    --font-sans: var(--font-jakarta), var(--font-kanit), ui-sans-serif, system-ui, sans-serif;
    --font-mono: var(--font-geist-mono), var(--font-kanit), ui-monospace, monospace;
    --font-heading: var(--font-sans);

    --radius-md: calc(var(--radius) * 0.8);
    --radius-lg: var(--radius);
    --radius-xl: calc(var(--radius) * 1.4);
}

:root {
    color-scheme: light;
    --radius: 0.625rem;
}

.dark {
    color-scheme: dark;
}
```

5. **Global base rules.** These carry much of the system's polish and belong in the base stylesheet:

```css
* {
    border-color: var(--border);
}
html {
    scrollbar-color: var(--border) transparent;
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
}
body {
    background: var(--background);
    color: var(--foreground);
}
:focus-visible {
    outline: 2px solid var(--ring);
    outline-offset: 2px;
}
input,
textarea {
    caret-color: var(--ring);
}
h1,
h2,
h3,
h4 {
    text-wrap: balance;
}
p,
li,
blockquote {
    text-wrap: pretty;
}
time,
.tabular {
    font-variant-numeric: tabular-nums;
}
::selection {
    background: var(--accent);
    color: var(--accent-foreground);
}
```

## Colors

A cool blue-grey neutral ramp carries the page, and three pale, saturated highlighters supply all of the colour.

### Primary

- **Highlighter Yellow** (#fff89c): The lead pen. Marks the hero phrase and the active desktop nav link, fills the active mobile nav tab, the scroll-to-top button and inline code in the light theme.

### Secondary

- **Highlighter Cyan** (#9ffaff): The structural pen. Marks second-level article headings, fills table headers and rings the small author avatar.

### Tertiary

- **Highlighter Pink** (#ff9fed): The expressive pen. Fills text selection and the lightbox close button, borders the large profile avatar, and tints callouts.
- **Pink Ink** (#a3237e, dark #ff9fed): The one colour that may be text. Post titles turn this on hover, it underlines article links, and it colours unordered list markers. The focus ring uses the same values.

### Neutral

- **Paper Grey** (#e0e5ec): The light-theme page, and the dark-theme text.
- **Graphite Ink** (#2a313a): The light-theme text, and the dark-theme page. It is also the fixed text colour on every highlighter fill in both themes.
- **Note Card** (#edf1f5, dark #343d48): Cards, panels, the footer, and the fill of inputs and pills.
- **Top Sheet** (#f4f6f9, dark #343d48): Popovers, chiefly the search dialog, which sits one tone above cards in the light theme.
- **Muted Wash** (#d2d8e0, dark #3b4652): Hover washes, selected search results, image placeholders, and zebra table rows at 50% opacity.
- **Soft Pencil** (#4b5562, dark #b8c0c9): Secondary text. This is the most frequently used text colour, covering summaries, dates and labels.
- **Hairline** (#b6bfc9, dark #4a5563): Dividers and container borders. It is decorative at 1.47:1 on the light page.
- **Control Outline** (#6f7a87, dark #808d9c): The outline of standalone controls, at 3.45:1 on the light page and 3.88:1 on the dark page.
- **Callout Tint** (`note-bg`): Highlighter Pink mixed into the card colour, at 22% in light and 12% into the page colour in dark.
- **Code Slate** (#2a313a, dark #1f252c) with **Code Text** (#e0e5ec): Code blocks stay dark in both themes.
- **Destructive** (`oklch` values in the frontmatter): Defined for error states on the base button. No live screen currently uses it.

### Named Rules

**The Three Highlighters Rule.** Yellow, cyan and pink are the only accents. There is no fourth colour and no gradient between them.

**The Ink On Pastel Rule.** A highlighter fill always carries Graphite Ink text, in both themes. Ink measures 11.98:1 on yellow, 11.00:1 on cyan and 7.15:1 on pink. This is why `ink` has no dark override.

**The Pens Are Not Pencils Rule.** Highlighters never set text on the light page, where they measure 1.15:1 (yellow), 1.06:1 (cyan) and 1.45:1 (pink). Use Pink Ink when a word needs colour. On the dark page the same pastels pass at 7:1 or better, and Pink Ink takes the pink highlighter's value there.

## Typography

**Display Font:** Plus Jakarta Sans (with Kanit for Thai, then the system sans stack)
**Body Font:** Plus Jakarta Sans (with Kanit for Thai, then the system sans stack)
**Label/Mono Font:** Geist Mono (with Kanit for Thai, then the system mono stack)

**Character:** A friendly geometric sans at every level, paired with a rounded Thai companion that shares its openness. The mono appears only for code and keyboard hints.

The source site registers Kanit's 300, 400, 500 and 600 files as CSS weights 400, 500, 600 and 700, so Thai renders one step lighter than its nominal weight. This is an observed implementation detail rather than a documented rule. Compare it against Jakarta in your own layouts before copying it.

### Hierarchy

- **Display** (bold, fluid 36 to 60px, 1.2, tight tracking): The home hero only.
- **Headline** (bold, fluid 30 to 42px, 1.25, tight tracking): Page titles and article titles.
- **Section Title** (bold, 24px, 1.35): Section headings on landing pages.
- **Feature Title** (bold, 26px, 1.3): The one featured post.
- **List Title** (bold, 20px, 1.4): Items in the full post list.
- **Row Title** (semibold, 16px, 1.45): Compact rows beside the featured post. Previous and next cards use the same treatment at 15px.
- **Prose H1 / H2 / H3** (28px bold, 24px bold, 19px semibold): Article headings, all with a scroll margin of 96px so the sticky header never covers an anchor. Only H2 carries the cyan marker.
- **Lede** (regular, 18px): The single sentence under the hero, limited to 38rem.
- **Body Prose** (regular, 16px, 1.85): Article paragraphs. List items and callouts tighten slightly to 1.8.
- **Body UI** (regular, 15px, 1.65): Summaries and page intros.
- **Label** (medium, 13px, 1.5): Dates, reading time and metadata rows.
- **Tag** (semibold, 12px, wide tracking, uppercase): Category tags and the small labels on previous and next cards.

### Named Rules

**The One Family Rule.** Headings and body share one sans family. Hierarchy comes from size, weight and tracking, never from a separate display face.

**The Tight Top Rule.** Negative tracking belongs to titles at section-title size and above. Smaller text keeps normal tracking, and only the small uppercase tag style opens up.

## Layout

The page is a single centred column of 64rem with no wider breakpoints, so large screens simply gain margin. Breakpoints are the Tailwind defaults at 640px, 768px and 1024px.

- **Shell:** Page gutters run 20px, widening to 32px at 640px. The header and footer use 24px, widening to 32px. Main content has 40px of vertical padding, widening to 56px. The header is 64px tall and sticky.
- **Widths inside the column:** 44rem for reading, 48rem for the hero, 42rem for page intros, 38rem for ledes, 28rem for the search field, 36rem for the search dialog.
- **Vertical rhythm:** Landing sections sit 80px apart, widening to 96px. Blocks within a page sit 40px apart, and related groups 16 to 24px.
- **Article rhythm:** Prose H1 has 48px above and 16px below, H2 56px and 16px, H3 40px and 12px. Paragraphs take 20px, while callouts, tables, code and images take 32px. Rules take 48px.
- **Grids:** The featured post and its list split 7fr to 5fr at 1024px. The profile page uses a 17rem sidebar beside fluid content at 768px, with the sidebar sticky at 96px from the top. Post rows gain an 8rem date column at 768px. Previous and next cards split into two columns at 640px.
- **Overflow:** The page itself never scrolls sideways. Tables, code blocks and the mobile filter rail scroll within their own bounds, and the filter rail bleeds to the screen edge by cancelling the gutter. Long words and URLs in prose wrap anywhere.
- **Stacking:** Header and scroll-to-top sit at 40, search at 50, lightbox at 100.

## Elevation & Depth

The system is flat at rest and builds depth through tone. On the light theme the sheets step lighter as they rise, from page to card to popover. On the dark theme card and popover share one raised tone. Translucency marks layers that float over content: the sticky header is the page colour at 80% with a 12px blur, the search overlay is ink at 70% with an 8px blur, and the lightbox overlay is ink at 95% with an 8px blur.

### Shadow Vocabulary

- **Avatar lift** (`box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)`): The large profile avatar. This is the one shadow on resting content.
- **Floating control** (`box-shadow: 0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)`): The scroll-to-top button.
- **Overlay** (`box-shadow: 0 25px 50px -12px rgb(0 0 0 / 0.25)`): The search dialog and the lightbox image.

### Named Rules

**The Flat Paper Rule.** Cards, panels, rows and inputs carry no shadow at rest. Separate them with tone and a hairline. Reserve shadow for overlays and floating controls.

## Shapes

The base radius is 0.625rem, and the scale derives from it. Corners are soft and never sharp, and there is no clipped or angled geometry.

- **Pills** (fully round): Filter chips, category links, tags, icon controls, pagination, mobile nav tabs, avatars.
- **Large soft corners** (14px): Containers and media, including panels, link cards, callouts, code blocks, images and the search dialog.
- **Medium soft corners** (10px): Search fields, table wrappers, contact rows and the focus outline around post rows.
- **Small soft corners** (8px): Inline code, search results and the copy button.
- **Tiny corners** (4px): Keyboard hints and the search dialog's close button.

Borders are always 1px, with two exceptions: the large avatar has a 4px pink border and the loading spinner a 2px border. Dashed hairlines mean empty states and appear nowhere else.

### Named Rules

**The Pill And Sheet Rule.** Compact controls and labels are pills. Anything that holds content, or that spans the full width of its parent such as a search field or contact row, is a rounded rectangle.

## Components

### Buttons

Compact, outlined and quiet, with colour arriving only on interaction.

- **Shape:** Fully round pills, 40px tall, with 16px side padding. Icon-only controls are 40px circles.
- **Default:** Note Card fill, Control Outline border, foreground text in the medium control style.
- **Hover:** Muted Wash fill.
- **Active filter chip:** The theme's foreground colour as the fill with the page colour as text, a solid inversion rather than an accent. Filter chips and category labels are uppercase.
- **Toned hover:** Footer social links and contact rows fill with a highlighter and ink text on hover, each link holding its own tone.
- **Theme toggle:** An outlined 40px circle whose border turns Highlighter Pink on hover. Icons are 18px line icons from Lucide.
- **Scroll-to-top:** A 44px Highlighter Yellow circle fixed to the bottom corner, inset 20px and widening to 32px. It appears after 400px of scrolling and lifts 2px on hover. This is the only live filled primary button.
- **Pagination:** Outlined pills with no fill. Disabled pills drop to 40% opacity.
- **Base button component:** A shadcn/ui button with `default`, `outline`, `secondary`, `ghost`, `destructive` and `link` variants exists underneath, at 36px with 8px corners, but only the theme toggle uses it, and that overrides it into a circle. Treat the pill as the live language, and restyle the base component to match rather than introducing its default shape.

### Chips

**Category Tag:** Non-interactive pill with a highlighter fill, ink text in the tag style, and a 1px ink border in the light theme that turns transparent in dark. Each content category keeps one tone everywhere, and unknown categories fall back to cyan.

**Category Link:** A pill control holding a 10px dot in the category's tone, the uppercase name, and a muted tabular count.

### Cards / Containers

- **Panel:** Note Card fill, Hairline border, large soft corners, 24px padding, no shadow. Used for the profile sidebar, with content centred.
- **Link Card:** The same surface at 20px padding. Its border strengthens to Control Outline on hover, its title turns Pink Ink, and its small arrow nudges 2px in the direction of travel.
- **Featured Post:** No container. A 16:9 image with large soft corners and a Muted Wash placeholder, then a metadata row, the feature title, and a summary clamped to three lines. The image scales to 1.02 on hover.
- **Post Row:** No container, separated from its neighbours by hairlines. The whole row is clickable through a stretched link. On hover the title turns Pink Ink. On keyboard focus the link's own outline is removed and the row takes a 2px ring outline at 4px offset.
- **Empty State:** A dashed Hairline border, centred small Soft Pencil text, and 48 to 64px of padding.

### Inputs / Fields

- **Search Field:** 44px tall with medium soft corners, Note Card fill and Control Outline border, with a 16px leading icon and room for a clear button. Text is 16px on small screens and 14px from 640px up.
- **Focus:** The border takes the ring colour, along with the global 2px outline. The caret also uses the ring colour.
- **Clear Button:** A 40px circle inside the field that fills Highlighter Pink on hover.

### Navigation

- **Header:** Sticky, translucent and blurred, with a bottom Hairline. A landscape logo sits left, with links and actions right.
- **Desktop Links:** Nav style in Soft Pencil, turning foreground on hover. The active link goes semibold with a yellow marker. A short vertical hairline separates links from actions.
- **Mobile:** There is no hamburger menu. Below 768px, a second header row spreads the links evenly as pills. The active tab fills Highlighter Yellow with ink text, and the others wash on hover.
- **Search Trigger:** A pill in the header with an icon, a label and a keyboard hint, collapsing to an icon-only 40px circle on small screens.
- **Footer:** Note Card fill with a top Hairline, holding brand text, text links and 40px social controls. It stacks and centres below 768px.
- **Skip Link:** Hidden until focused, then a yellow pill with ink text at the top left.

### Marker Highlight (Signature)

A highlighter stroke that covers the lower part of the text in light mode and becomes a thick coloured underline in dark mode. It wraps cleanly across lines. Use it for the hero phrase, with animation, for the active desktop nav link in yellow, and for second-level article headings in cyan.

```css
.marker {
    background-image: linear-gradient(var(--mk), var(--mk));
    background-repeat: no-repeat;
    background-size: 100% 0.42em;
    background-position: 0 88%;
    box-decoration-break: clone;
    -webkit-box-decoration-break: clone;
    padding-inline: 0.06em;
}

.dark .marker {
    background-image: none;
    text-decoration-line: underline;
    text-decoration-color: var(--mk);
    text-decoration-thickness: 0.14em;
    text-underline-offset: 0.16em;
    text-decoration-skip-ink: auto;
}

.marker-yellow {
    --mk: var(--marker-yellow);
}
.marker-cyan {
    --mk: var(--marker-cyan);
}
.marker-pink {
    --mk: var(--marker-pink);
}

@media (prefers-reduced-motion: no-preference) {
    .marker-animate {
        animation: marker-swipe 900ms cubic-bezier(0.16, 1, 0.3, 1) 200ms both;
    }
}

@keyframes marker-swipe {
    from {
        background-size: 0% 0.42em;
    }
}
```

**The Pen Changes In The Dark Rule.** A marker is a background swipe on light surfaces and an underline on dark ones, because a pale swipe behind light text would destroy its contrast.

### Article Content

- **Links:** Foreground text at medium weight with a thin Pink Ink underline (0.08em thick, 0.22em offset) that thickens to 0.16em on hover. External links add a small icon.
- **Lists:** Unordered markers are Pink Ink and ordered markers are medium-weight Soft Pencil.
- **Callout:** The Callout Tint fill with large soft corners and no border or icon. All blockquotes render this way.
- **Table:** A Hairline-bordered wrapper with medium soft corners on Note Card, scrolling on overflow. Headers are Highlighter Cyan with semibold ink text at 14px. Columns have vertical dividers, using ink at 15% in the header and Hairline in the body. Even rows wash at 50% Muted, and cells use tabular numerals.
- **Code Block:** Code Slate with large soft corners, capped at 480px tall with a thin scrollbar. The dark theme adds a Hairline border.
- **Copy Button:** A 32px square in the block's top right, with a translucent white fill and border at 10% and 15%. On pointer devices it appears when the block is hovered or the button is focused, and it is always visible on touch. It fills yellow with ink text on hover.
- **Inline Code:** Yellow fill with ink text in light, Muted Wash fill with yellow text in dark.
- **Images:** Large soft corners with a Hairline border, dimming to 95% opacity on hover. They open in the lightbox.

### Search Dialog

Opened with a keyboard shortcut and anchored near the top of the screen rather than centred, at 64px on small screens and 112px from 640px. It has a Top Sheet surface, a Hairline border that strengthens while the input is focused, and the overlay shadow. The input row uses 16px text and takes a 60% Muted Wash while focused. Results have small soft corners and 12px padding. The selected result takes Muted Wash and an inset 1px Control Outline ring. A Muted footer lists keyboard hints, and the loading spinner has a pink top edge.

### Image Lightbox

A nearly opaque ink overlay with a blur, fading in over 150ms. The image has medium soft corners and the overlay shadow. The close button is a Highlighter Pink circle that turns yellow on hover, with a cyan focus outline for contrast against the overlay.

### Motion

No animation library is used, and no effects are tied to scroll position.

- **Default:** Colour transitions at Tailwind's 150ms with `cubic-bezier(0.4, 0, 0.2, 1)`.
- **200ms:** The scroll-to-top button's fade and lift, and the page shell's colour change.
- **500ms:** The featured image's scale on hover.
- **900ms:** The hero marker swipe, with a 200ms delay and an expressive ease-out.
- **Theme switch:** Transitions are disabled during the swap, so the whole page changes at once.
- **Press:** Base buttons move down 1px while active.
- **Reduced motion:** The marker swipe and lightbox fade only run when motion is allowed, the featured image stops scaling smoothly, and scroll-to-top jumps rather than scrolling.

## Do's and Don'ts

### Do:

- **Do** set ink text on every highlighter fill in both themes (The Ink On Pastel Rule).
- **Do** use Pink Ink when text needs colour on the light theme.
- **Do** define every colour for both themes, applying `-dark` values to the same variable inside `.dark`.
- **Do** keep article text to a 44rem column at 1.85 line height.
- **Do** keep each content category on one highlighter tone everywhere it appears.
- **Do** use Control Outline for standalone controls and Hairline for dividers and containers.
- **Do** keep tap targets at 40px or larger. The 32px copy button is the existing exception, because it sits within a code block.
- **Do** make whole rows and cards clickable with a stretched link, and move the focus outline to the container.
- **Do** let tables, code and filter rails scroll inside their own bounds rather than widening the page.
- **Do** honour `prefers-reduced-motion` for the marker, image scaling and smooth scrolling.

### Don't:

- **Don't** use highlighter colours or `primary` as text on the light theme. The base button's `link` variant does this and measures 1.15:1, so restyle it before use.
- **Don't** add a fourth accent, or blend the highlighters into a gradient.
- **Don't** put a marker swipe behind light text in the dark theme.
- **Don't** add shadows to resting cards, panels, rows or inputs.
- **Don't** introduce a separate display typeface.
- **Don't** rely on Hairline alone to identify a standalone control. It measures 1.47:1 on the light page.
- **Don't** spread the marker widely. It belongs on the hero phrase, the active nav link and second-level article headings.
- **Don't** use dashed borders for anything except empty states.
