# Cranpose UI layout

Layout contracts and policies shared by Cranpose UI components. `Constraints`
express minimum and maximum width and height in logical pixels; `Measurable`
and `Placeable` describe the parent-child measurement and placement protocol.
The crate also provides alignment, arrangement, intrinsic measurement and
pixel helpers.

Use these types for a custom measure policy or layout modifier.
Most application code can use the layouts re-exported by `cranpose`.
