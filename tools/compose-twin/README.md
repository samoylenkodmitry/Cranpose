# Compose twin

Compose Desktop renderings of desktop-demo scenes, written with the same
composables, modifier chains and constants as the Rust code in
`apps/desktop-demo/src/app.rs`. `robot_compose_twin_scenes` renders the Rust
side and fails when a scene strays from its frame in `reference/`.

## Checking Cranpose against the frames

```bash
./run_robot_test.sh --example robot_compose_twin_scenes
```

A pixel strays when it differs by more than `TWIN_STRAY_DELTA` where the
Compose frame has no edge; a scene may have `TWIN_STRAY_LIMIT` of them
(`apps/desktop-demo/src/test_screens/compose_twin.rs`). Glyph and corner
antialiasing only move edge pixels, and a matching scene has a handful of
stray ones; an element moved by a pixel leaves over a hundred.

## Changing a scene

Change the Rust scene and `src/main/kotlin/dev/cranpose/twin/Scenes.kt`
together, then render the frames again (JDK 17):

```bash
cd tools/compose-twin && ./gradlew renderScenes
```

`-PoutDir=<dir>` renders them somewhere else instead of over `reference/`.

## The modifier matrix

`matrix.toml` describes small probes: every ordered chain of a few
modifiers (padding, background, size, fill, offset, clip, alpha), the same
chains around a line of text, rows and columns at every arrangement and
alignment, box alignments and weighted rows. `generate_matrix.py` turns it
into the same scenes on both sides, `compose_twin_matrix.rs` and
`Matrix.kt`; never edit those by hand. `just twin-matrix-check` fails when
they are stale.

Every frame is laid out and captured at each density the description
names, 1 and 2.625, so lengths that round to device pixels (a 4 point
padding is 10.5 pixels at 2.625) are held to Compose's rounding; a frame
beyond density 1 is named `<frame>@<density>`. Text stays at density 1.

Each matrix frame is a grid of cells compared one by one. Solid shapes on
whole pixels must match exactly; text cells skip the Compose frame's edges
and may stray by `TWIN_CELL_STRAY_LIMIT`. Cells known to differ are listed
in `matrix-baseline.txt`: the robot fails on any other cell that differs,
and on a listed cell that matches again, so the list only shrinks.

After changing the description:

```bash
python3 tools/compose-twin/generate_matrix.py
cd tools/compose-twin && ./gradlew renderScenes
```
