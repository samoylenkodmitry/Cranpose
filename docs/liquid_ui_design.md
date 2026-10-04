# Liquid UI

`cranpose-liquid` supplies glass materials, themes, motion and UI components.
Applications use the crate directly or the `cranpose::liquid` namespace. The
[public exports](../crates/cranpose-liquid/src/lib.rs) define the current API.

## Theme and material

`LiquidTheme` provides semantic colors, typography and glass defaults to a
composition subtree. `LiquidThemeSpec` selects the accent and appearance mode.
`SchemeMode::Auto` follows the system appearance; `Light` and `Dark` select a
fixed palette. See [theme.rs](../crates/cranpose-liquid/src/theme.rs).

`Glass` describes a material. `LiquidModifierExt` applies the material to a
modifier. Material options control the shape, tint, refraction, edge light,
shadow and deformation. The renderer resolves the required backdrop inputs
and draws the effect through the shared render graph. See
[material.rs](../crates/cranpose-liquid/src/material.rs) and
[render architecture](render_arch.md).

## Components

| Group | Public components |
| --- | --- |
| Surfaces and actions | `GlassSurface`, `GlassButton`, `GlassIconButton`, `GlassIconButtonGroup` |
| Value controls | `LiquidToggle`, `LiquidSlider`, `LiquidSegmentedControl` |
| Chips | `LiquidChip`, `LiquidActionChip` |
| Lists | `LiquidCard`, `LiquidListSection`, `LiquidListRow` |
| Navigation | `LiquidTabBar`, `LiquidTabBarWithAccessory`, `LiquidNavBar` |
| Menus | `LiquidMenu`, `LiquidDropdownMenu` |
| Text input | `LiquidSearchField` |

Each component's source defines its arguments and behavior. The
[widgets module](../crates/cranpose-liquid/src/widgets/mod.rs) lists the public
exports. The [application guide](guide.md#liquid-components) contains examples.

## Motion and reference checks

Components use the shared animation runtime and the Liquid motion and dynamics
APIs. Springs preserve value-space velocity across target changes. The
[dynamics](../crates/cranpose-liquid/src/dynamics.rs) and
[motion](../crates/cranpose-liquid/src/motion.rs) modules define the common controls.

The desktop demo's Liquid tab exercises component input and material settings.
[Native reference tools](../apps/liquid-reference/README.md) provide capture
and comparison workflows. Use the [pixel contract](liquid_scroll_pixel_stability.md)
and [device protocol](device_measurement.md) for renderer changes. The actual
pass and capture costs depend on the scene and target device.
