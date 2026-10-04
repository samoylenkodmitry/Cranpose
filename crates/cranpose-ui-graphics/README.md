# Cranpose UI graphics

Geometry, colors, units and draw data for Cranpose. The crate defines
`Color`, `Point`, `Size`, `Rect`, `Dp`, `Sp`, brushes, paths, shapes and
render-effect types shared by layout, widgets and renderer backends.

Use these types when a custom widget or renderer needs shared draw data.
The `cranpose-ui-graphics::prelude` exports the common color, geometry, image,
pointer-icon, stroke and unit types.
