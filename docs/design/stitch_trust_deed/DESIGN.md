# Design System Strategy: The Financial Architect

## 1. Overview & Creative North Star
The Creative North Star for this design system is **"The Precision Vault."** 

In high-end financial environments, luxury is defined by the absence of noise and the presence of absolute clarity. This system moves away from the "SaaS-standard" look of boxed-in widgets and heavy borders. Instead, it adopts an editorial, data-centric layout that feels more like a custom Bloomberg terminal reimagined by a Swiss design house. 

We break the "template" look through **intentional asymmetry** and **tonal depth**. By utilizing a horizontal hierarchy, we guide the eye through complex datasets using a "weighted flow" approach—where the most critical figures (Net Flow) act as anchors, and secondary data orbits them in a spacious, breathable environment.

---

## 2. Colors
This system leverages a monochromatic charcoal foundation to allow the vibrant emerald and crimson accents to "vibrate" against the dark canvas, ensuring immediate cognitive recognition of financial health.

### The "No-Line" Rule
Traditional dashboards rely on 1px borders to separate cards. This design system **prohibits 1px solid borders for sectioning.** Boundaries are defined strictly through background color shifts. For instance, a `surface_container_highest` widget should sit atop a `surface_dim` background. The change in hex value is the boundary.

### Surface Hierarchy & Nesting
Treat the UI as a series of physical layers. Each layer represents a level of data granularity:
*   **Base Layer (`surface` / `#111417`):** The canvas.
*   **Section Layer (`surface_container_low` / `#191c1f`):** Grouping major dashboard modules.
*   **Interactive Layer (`surface_container_highest` / `#323538`):** The most prominent cards or "active" states.

### The "Glass & Gradient" Rule
To add soul to the "Precision Vault," use **Glassmorphism** for floating overlays (e.g., dropdowns, tooltips). Use `surface_variant` with a 60% opacity and a `20px` backdrop-blur. 

### Signature Textures
For Primary CTAs or high-value growth indicators, use a subtle linear gradient:
*   **Emerald Pulse:** `primary` (#4edea3) to `primary_container` (#10b981) at 135 degrees. This prevents the green from looking "flat" and adds a sense of light-source depth.

---

## 3. Typography
We use a dual-typeface system to balance high-end editorial feel with technical precision.

*   **Display & Headlines (Manrope):** Chosen for its wide, modern stance. Use `display-lg` and `headline-md` for portfolio totals. The geometry of Manrope conveys stability and institutional trust.
*   **Data & Body (Inter / Roboto Mono):** Use Inter for UI labels and Roboto Mono (optional variant) for ticking price data. 
*   **Hierarchy:** Use `label-sm` in `on_surface_variant` for metadata. By keeping labels small and dimmed, the "Big Numbers" in `title-lg` command the user’s attention immediately.

---

## 4. Elevation & Depth
Depth is a tool for focus, not just decoration.

### The Layering Principle
Stacking tiers is the primary method of elevation. A "Search" bar should be `surface_container_lowest` (inset) to feel like a carved-out utility, while an "Alert" card should be `surface_container_high` (elevated) to feel like a notification pinned to the top.

### Ambient Shadows
For floating elements, use "Ambient Shadows":
*   `box-shadow: 0px 24px 48px rgba(0, 0, 0, 0.4);`
*   Avoid grey shadows. The shadow should feel like a deep void underneath the component, achieved by using the `surface_container_lowest` color as the shadow base.

### The "Ghost Border" Fallback
If contrast is required for accessibility (e.g., in high-glare environments), use a **Ghost Border**:
*   `border: 1px solid rgba(134, 148, 138, 0.15);` (using `outline_variant` at 15% opacity).

### Soft Glows
To emphasize timeline "dots" or active statuses, apply a `4px` blur of the `primary` (emerald) or `secondary` (crimson) color behind the element. This creates a "luminescent" effect that mimics high-end hardware displays.

---

## 5. Components

### Cards & Modules
*   **Style:** No borders. Use `surface_container_low` background with a `xl` (0.75rem) roundedness.
*   **Separation:** Forbid dividers. Use **Vertical White Space** (32px or 48px) to separate content blocks.

### Buttons
*   **Primary:** `primary` background, `on_primary` text. No border. `md` roundedness.
*   **Secondary (Ghost):** No background. `outline` border (Ghost Style).
*   **States:** On hover, primary buttons should transition to `primary_fixed_dim` with a soft emerald glow.

### Financial Chips
*   **Positive Flow:** `primary_container` background with `on_primary_container` text.
*   **Negative Flow:** `secondary_container` background with `on_secondary_container` text.
*   **Shape:** `full` (pill-shaped) to contrast against the architectural squareness of the dashboard.

### Data Inputs
*   **Style:** `surface_container_lowest` background. 
*   **Active State:** No thick border change. Instead, change the background to `surface_container_highest` and add a `primary` (emerald) 1px bottom-stroke only.

---

## 6. Do's and Don'ts

### Do:
*   **Do** prioritize "negative space" as a functional element. It reduces cognitive load in data-heavy screens.
*   **Do** use `primary_fixed` for small "win" indicators (e.g., +2.4%) to ensure they pop against the dark surface.
*   **Do** use `manrope` for any text larger than 18px to maintain the editorial feel.

### Don't:
*   **Don't** use pure white (#FFFFFF) for text. Always use `on_surface` (#e1e2e7) to prevent eye strain in dark mode.
*   **Don't** use standard "drop shadows" with 20%+ opacity. They feel heavy and "un-designed."
*   **Don't** use lines to separate list items. Use a slight `surface` color shift on hover to indicate row selection.
*   **Don't** crowd the horizontal hierarchy. If a row has more than 5 data points, use a "Secondary Detail" drawer rather than squishing columns.