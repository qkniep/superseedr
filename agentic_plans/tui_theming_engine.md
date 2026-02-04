# TUI Theming Engine Design Doc

**Status**: Draft

**Summary**
Create an extensible theming foundation for the TUI that preserves all current colors as the default theme, while enabling multiple built-in themes (e.g., Neon and Candy Land Pink) and optional effects (glow/flicker) without requiring component-level color changes.

**Goals**
- Preserve current visuals by mapping every existing color into a default theme with identical RGB values.
- Centralize color access and (optional) effects in a theme layer.
- Enable additional built-in themes that can override the entire palette.
- Keep the system lightweight and maintainable.

**Non-Goals**
- Achieving final “perfect” visual polish for Neon or Candy Land Pink.
- Building a CSS-like cascade or selector system.
- Overhauling layout or UI composition.

**Current State**
- Colors are hardcoded as constants in `src/theme.rs` and referenced widely via `theme::COLOR`.
- Components directly choose colors for semantic and non-semantic use cases.
- No central concept of a theme beyond the constants file.

**Proposed Design**
- Replace global color constants with a `Theme` struct that exposes semantic and scale slots.
- Add a `ThemeName` enum for built-ins.
- Add optional `ThemeEffects` that can be used to compute glow/flicker styles later.
- Store the resolved theme in app state and pass it into all drawing/formatting paths.
- Components request palette colors from the theme; optional modifiers (bold/underline) remain component-controlled.

**Theme Schema (Initial)**
The theme is split into **semantic** and **scale** slots. Semantic slots describe intent (text, surfaces, errors), while scale slots are for gradients and categorical palettes.

**Semantic Slots (Initial)**
- `text`
- `subtext0`
- `subtext1`
- `overlay0`
- `surface0`
- `surface1`
- `surface2`
- `border`
- `white`

**Scale Slots (Initial)**
- `speed` (ordered gradient array used by speed tiers)
- `ip_hash` (categorical array used to color IPs deterministically)
- `heatmap` (low/medium/high/empty)
- `stream` (inflow/outflow for block stream visuals)
- `dust` (foreground/midground/background for parallax dust)
- `categorical` (legacy categorical palette used across components)

**Theme Effects (Initial, No Behavioral Change)**
- `glow_enabled: bool`
- `flicker_hz: f32`
- `flicker_intensity: f32`
These fields exist for extensibility but are not required to affect rendering in the first iteration.
Effects should be addable later without refactoring if components always source colors through the theme instance.

**Theme Selection**
- Introduce a `ui_theme` config field in settings to select a built-in theme.
- Default remains Catppuccin Mocha to preserve existing visuals.
- TUI config screen integration can be deferred.

**Neon Theme Strategy**
- Provide a complete semantic + scale palette with neon-leaning colors for every slot.
- Effects can be enabled later (glow/flicker) without changing component code.

**Candy Land Pink Strategy**
- Provide a complete semantic + scale palette for every slot.
- Gradients (e.g. `speed`) can be white-to-pink without requiring any red hues.
- Effects should be off or minimal by default.

**Migration Plan**
1. Add the `Theme` struct and built-in registry in `src/theme.rs`.
2. Replace direct `theme::COLOR` usage with `theme.COLOR` via a theme instance passed through rendering code.
3. Add `ui_theme` to settings and load it into app state.
4. Add Neon and Candy Land Pink palettes as built-ins.

**Risks**
- The refactor touches many call sites; mechanical replacements must avoid logic changes.
- Some components may currently hardcode color intent; these should be mapped to equivalent slots rather than redesigned.
- Compile-time completeness means new slots require all built-in themes to be updated, which is desired but adds maintenance cost.

**Performance Considerations**
- Structural theming (palette lookup) is negligible overhead.
- Future effects (glow/flicker) add small per-style computation and may increase redraw frequency if animation is enabled.

**Color Reuse Policy**
- Prefer shared palette slots across components for consistency and manageability.
- Only introduce component-specific slots when shared roles are insufficient or visually incorrect.

**Rollout**
- Phase 1: Structural refactor + default theme parity.
- Phase 2: Add built-in palettes (Neon, Candy Land Pink) without effects.
- Phase 3: Introduce effects and tune per-theme behavior if desired.

**Open Questions**
- Should theme selection be exposed in the TUI config screen immediately?
- Should the theme system remain one-to-one with palette slots, or move toward semantic role names later?
- Which components (if any) should be allowed to override theme-provided colors?
