#!/usr/bin/env python3
"""Generates the modifier matrix (issue #905, section 3) for both frameworks.

Reads `matrix.toml` beside this script and writes the Cranpose scenes to
`apps/desktop-demo/src/test_screens/compose_twin_matrix.rs` and the Compose
ones to `src/main/kotlin/dev/cranpose/twin/Matrix.kt`. With `--check` it
writes nothing and fails when either file is not what the description
generates, which is how CI keeps the two halves from drifting apart.
"""

from __future__ import annotations

import argparse
import itertools
import re
import subprocess
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
DESCRIPTION = HERE / "matrix.toml"
RUST_OUT = ROOT / "apps/desktop-demo/src/test_screens/compose_twin_matrix.rs"
RUST_EDITION = "2021"
KOTLIN_OUT = HERE / "src/main/kotlin/dev/cranpose/twin/Matrix.kt"

BLOCK_COLORS = ["red", "blue", "gold", "green"]


@dataclass(frozen=True)
class Cell:
    """One probe: its name and the call that draws it on each side."""

    name: str
    rust: str
    kotlin: str



@dataclass(frozen=True)
class Family:
    """Cells compared the same way, paged into frames, at `densities`."""

    name: str
    exact: bool
    cells: list[Cell]
    densities: tuple[float, ...] = ()


def rust_float(value: float) -> str:
    text = repr(float(value))
    return text if "." in text or "e" in text else f"{text}.0"


def kotlin_float(value: float) -> str:
    return f"{rust_float(value)}f"


def constant(name: str) -> str:
    return name.upper()


def kotlin_color(name: str) -> str:
    return name.capitalize()


def chain_family(spec: dict, ops: dict) -> Family:
    text = spec.get("text")
    cells = []
    for chain in itertools.permutations(spec["ops"], spec["length"]):
        rust_chain = "".join(f".{ops[op]['rust']}" for op in chain)
        kotlin_chain = "".join(f".{ops[op]['kotlin']}" for op in chain)
        if text is None:
            rust = f"probe(Modifier::empty(){rust_chain})"
            kotlin = f"Probe(Modifier{kotlin_chain})"
        else:
            rust = f'Text("{text}", Modifier::empty(){rust_chain}, TextStyle::default())'
            kotlin = f'Text("{text}", Modifier{kotlin_chain})'
        cells.append(Cell(" ".join(chain), rust, kotlin))
    return Family(spec["name"], text is None, cells, tuple(spec.get("densities", ())))


def line_family(description: dict, axis: str) -> Family:
    """Each arrangement against each cross-axis alignment."""
    row = axis == "row"
    alignments = description["row_alignments" if row else "column_alignments"]
    cells = []
    for (arrangement, arranged), (alignment, aligned) in itertools.product(
        description["arrangements"].items(), alignments.items()
    ):
        kotlin_arrangement = arranged["kotlin"]
        if not row:
            kotlin_arrangement = {"Start": "Top", "End": "Bottom"}.get(
                kotlin_arrangement, kotlin_arrangement
            )
        rust = f"{axis}_probe({arranged['rust']}, {aligned['rust']})"
        kotlin = (
            f"{axis.capitalize()}Probe(Arrangement.{kotlin_arrangement}, "
            f"Alignment.{aligned['kotlin']})"
        )
        cells.append(Cell(f"{axis} {arrangement} {alignment}", rust, kotlin))
    return Family(f"{axis}s", True, cells)


def box_family(description: dict) -> Family:
    cells = [
        Cell(
            f"box {name}",
            f"box_probe({aligned['rust']})",
            f"BoxProbe(Alignment.{aligned['kotlin']})",
        )
        for name, aligned in description["box_alignments"].items()
    ]
    return Family("boxes", True, cells)


def weight_family(description: dict) -> Family:
    cells = []
    for index, children in enumerate(description["weights"]["rows"]):
        rust_children = ", ".join(
            f"({rust_float(weight)}, {'true' if fill else 'false'})" for weight, fill in children
        )
        kotlin_children = ", ".join(
            f"{kotlin_float(weight)} to {'true' if fill else 'false'}" for weight, fill in children
        )
        cells.append(
            Cell(
                f"weights {index}",
                f"weights_probe(&[{rust_children}])",
                f"WeightsProbe(listOf({kotlin_children}))",
            )
        )
    return Family("weights", True, cells)


def families(description: dict) -> list[Family]:
    ops = description["ops"]
    default = tuple(description["densities"])
    return [
        family if family.densities else Family(family.name, family.exact, family.cells, default)
        for family in [
            *(chain_family(spec, ops) for spec in description["chains"]),
            line_family(description, "row"),
            line_family(description, "column"),
            box_family(description),
            weight_family(description),
        ]
    ]


@dataclass(frozen=True)
class Frame:
    name: str
    function: str
    exact: bool
    cells: list[Cell]
    density: float


def frames(description: dict) -> list[Frame]:
    """Every frame at every density of its family; a page's frames at
    several densities share its function."""
    grid = description["grid"]
    per_frame = grid["columns"] * grid["rows"]
    result = []
    for family in families(description):
        for page, start in enumerate(range(0, len(family.cells), per_frame)):
            for density in family.densities:
                suffix = "" if density == 1.0 else f"@{density:g}"
                result.append(
                    Frame(
                        name=f"matrix-{family.name}-{page:02}{suffix}",
                        function=f"{family.name}_{page:02}",
                        exact=family.exact,
                        cells=family.cells[start : start + per_frame],
                        density=density,
                    )
                )
    return result


def pages(all_frames: list[Frame]) -> list[Frame]:
    """One frame per function, for the code that draws it."""
    seen = {}
    for frame in all_frames:
        seen.setdefault(frame.function, frame)
    return list(seen.values())


HEADER = (
    "Generated by `tools/compose-twin/generate_matrix.py` from\n"
    "`tools/compose-twin/matrix.toml`. Do not edit: change the description and\n"
    "generate again."
)


def rows_of(frame: Frame, columns: int) -> list[list[Cell]]:
    return [frame.cells[at : at + columns] for at in range(0, len(frame.cells), columns)]


def rust_source(description: dict, all_frames: list[Frame]) -> str:
    grid = description["grid"]
    width = rust_float(grid["cell_width"])
    height = rust_float(grid["cell_height"])
    out = [f"//! {line}" if line else "//!" for line in HEADER.split("\n")]
    out += [
        "//!",
        "//! The Cranpose half of the modifier matrix (issue #905): each frame",
        "//! is a grid of cells, one probe each, compared cell by cell with the",
        "//! Compose frame in `tools/compose-twin/reference`.",
        "",
        "use cranpose_ui::{",
        "    composable, Alignment, BoxSpec, Color, Column, ColumnSpec, GraphicsLayer,",
        "    HorizontalAlignment, LinearArrangement, Modifier, Row, RowSpec, Text, TextStyle,",
        "    VerticalAlignment,",
        "};",
        "",
        "use super::compose_twin::{TwinGrid, TwinScene, TwinTolerance};",
        "",
    ]
    for name, (r, g, b, a) in description["colors"].items():
        out.append(
            f"const {constant(name)}: Color = Color({rust_float(r)}, {rust_float(g)}, "
            f"{rust_float(b)}, {rust_float(a)});"
        )
    out.append(
        "const BLOCK_COLORS: [Color; "
        f"{len(BLOCK_COLORS)}] = [{', '.join(constant(c) for c in BLOCK_COLORS)}];"
    )
    out += [
        "",
        "/// Where every matrix frame lays its cells.",
        "pub const MATRIX_GRID: TwinGrid = TwinGrid {",
        f"    origin: {grid['origin']},",
        f"    columns: {grid['columns']},",
        f"    cell_width: {grid['cell_width']},",
        f"    cell_height: {grid['cell_height']},",
        "};",
        "",
        "/// Every matrix frame, in the order the Compose twin renders them.",
        f"pub const MATRIX_FRAMES: [TwinScene; {len(all_frames)}] = [",
    ]
    for frame in all_frames:
        out += [
            "    TwinScene {",
            f'        name: "{frame.name}",',
            f"        content: {frame.function},",
            f"        cells: {frame.function.upper()}_CELLS,",
            f"        tolerance: TwinTolerance::{'Exact' if frame.exact else 'Edges'},",
            f"        density: {rust_float(frame.density)},",
            "    },",
        ]
    out.append("];")
    for frame in pages(all_frames):
        names = ", ".join(f'"{cell.name}"' for cell in frame.cells)
        out += ["", f"const {frame.function.upper()}_CELLS: &[&str] = &[{names}];"]
    out += [
        "",
        "/// The block every chain probe wraps.",
        "#[composable]",
        "fn probe(modifier: Modifier) {",
        "    cranpose_ui::Box(modifier, BoxSpec::default(), || {",
        "        block(20.0, 14.0, GREEN);",
        "    });",
        "}",
        "",
        "#[composable]",
        "fn block(width: f32, height: f32, color: Color) {",
        "    cranpose_ui::Box(",
        "        Modifier::empty().size_points(width, height).background(color),",
        "        BoxSpec::default(),",
        "        || {},",
        "    );",
        "}",
        "",
        "/// Three blocks in a dim row, arranged and aligned as given.",
        "#[composable]",
        "fn row_probe(arrangement: LinearArrangement, alignment: VerticalAlignment) {",
        "    Row(",
        "        Modifier::empty().size_points(121.0, 65.0).background(DIM),",
        "        RowSpec::new()",
        "            .horizontal_arrangement(arrangement)",
        "            .vertical_alignment(alignment),",
        "        || {",
        "            block(16.0, 12.0, RED);",
        "            block(22.0, 30.0, BLUE);",
        "            block(12.0, 20.0, GOLD);",
        "        },",
        "    );",
        "}",
        "",
        "/// The same blocks, turned, in a dim column.",
        "#[composable]",
        "fn column_probe(arrangement: LinearArrangement, alignment: HorizontalAlignment) {",
        "    Column(",
        "        Modifier::empty().size_points(89.0, 97.0).background(DIM),",
        "        ColumnSpec::new()",
        "            .vertical_arrangement(arrangement)",
        "            .horizontal_alignment(alignment),",
        "        || {",
        "            block(12.0, 16.0, RED);",
        "            block(30.0, 22.0, BLUE);",
        "            block(20.0, 12.0, GOLD);",
        "        },",
        "    );",
        "}",
        "",
        "/// A block in a dim box at `alignment`.",
        "#[composable]",
        "fn box_probe(alignment: Alignment) {",
        "    cranpose_ui::Box(",
        "        Modifier::empty().size_points(121.0, 91.0).background(DIM),",
        "        BoxSpec::default().content_alignment(alignment),",
        "        || block(30.0, 20.0, RED),",
        "    );",
        "}",
        "",
        "/// A dim row of `(weight, fill)` children; weight 0 is a fixed block.",
        "#[composable]",
        "fn weights_probe(children: &'static [(f32, bool)]) {",
        "    Row(",
        "        Modifier::empty().size_points(121.0, 40.0).background(DIM),",
        "        RowSpec::new(),",
        "        move || {",
        "            for (at, &(weight, fill)) in children.iter().enumerate() {",
        "                let color = BLOCK_COLORS[at % BLOCK_COLORS.len()];",
        "                cranpose_core::with_key(&at, || {",
        "                    if weight == 0.0 {",
        "                        block(18.0, 24.0, color);",
        "                    } else {",
        "                        cranpose_ui::Box(",
        "                            Modifier::empty()",
        "                                .weight_with_fill(weight, fill)",
        "                                .width(10.0)",
        "                                .height(24.0)",
        "                                .background(color),",
        "                            BoxSpec::default(),",
        "                            || {},",
        "                        );",
        "                    }",
        "                });",
        "            }",
        "        },",
        "    );",
        "}",
    ]
    out += [
        "",
        "/// Lays `cells` out on [`MATRIX_GRID`], each in its own clipped cell.",
        "#[composable]",
        "fn grid(cells: &'static [fn()]) {",
        f"    Column(Modifier::empty().padding({rust_float(grid['origin'])}), ColumnSpec::new(), move || {{",
        f"        for (row_index, row) in cells.chunks({grid['columns']}).enumerate() {{",
        "            cranpose_core::with_key(&row_index, || {",
        "                Row(Modifier::empty(), RowSpec::new(), move || {",
        "                    for (index, cell) in row.iter().enumerate() {",
        "                        cranpose_core::with_key(&index, || {",
        "                            cranpose_ui::Box(",
        f"                                Modifier::empty().size_points({width}, {height}).clip_to_bounds(),",
        "                                BoxSpec::default(),",
        "                                move || cell(),",
        "                            );",
        "                        });",
        "                    }",
        "                });",
        "            });",
        "        }",
        "    });",
        "}",
    ]
    for frame in pages(all_frames):
        names = [f"{frame.function}_{index:02}" for index in range(len(frame.cells))]
        out += [
            "",
            "#[composable]",
            f"fn {frame.function}() {{",
            f"    grid(&[{', '.join(names)}]);",
            "}",
        ]
        for name, cell in zip(names, frame.cells):
            out += ["", "#[composable]", f"fn {name}() {{", f"    {cell.rust};", "}"]
    return "\n".join(out) + "\n"


def kotlin_source(description: dict, all_frames: list[Frame]) -> str:
    grid = description["grid"]
    out = [f"// {line}" for line in HEADER.split("\n")]
    out += [
        "//",
        "// The Compose half of the modifier matrix (issue #905).",
        "",
        "package dev.cranpose.twin",
        "",
        "import androidx.compose.foundation.background",
        "import androidx.compose.foundation.layout.Arrangement",
        "import androidx.compose.foundation.layout.Box",
        "import androidx.compose.foundation.layout.Column",
        "import androidx.compose.foundation.layout.Row",
        "import androidx.compose.foundation.layout.fillMaxWidth",
        "import androidx.compose.foundation.layout.height",
        "import androidx.compose.foundation.layout.offset",
        "import androidx.compose.foundation.layout.padding",
        "import androidx.compose.foundation.layout.size",
        "import androidx.compose.foundation.layout.width",
        "import androidx.compose.runtime.Composable",
        "import androidx.compose.ui.Alignment",
        "import androidx.compose.ui.Modifier",
        "import androidx.compose.ui.draw.clipToBounds",
        "import androidx.compose.ui.graphics.Color",
        "import androidx.compose.ui.graphics.graphicsLayer",
        "import androidx.compose.ui.unit.dp",
        "",
    ]
    for name, (r, g, b, a) in description["colors"].items():
        out.append(
            f"private val {kotlin_color(name)} = Color({kotlin_float(r)}, {kotlin_float(g)}, "
            f"{kotlin_float(b)}, {kotlin_float(a)})"
        )
    out += [
        "",
        "/** Every matrix frame, in the order Cranpose captures them. */",
        "val MATRIX_FRAMES: List<TwinFrame> = listOf(",
    ]
    out += [
        f'    TwinFrame("{frame.name}", {kotlin_float(frame.density)}) {{ {frame.function}() }},'
        for frame in all_frames
    ]
    out += [
        ")",
        "",
        "@Composable",
        "private fun Probe(modifier: Modifier) {",
        "    Box(modifier) { Block(20, 14, Green) }",
        "}",
        "",
        "@Composable",
        "private fun Block(width: Int, height: Int, color: Color) {",
        "    Box(Modifier.size(width.dp, height.dp).background(color))",
        "}",
        "",
        "@Composable",
        "private fun RowProbe(arrangement: Arrangement.Horizontal, alignment: Alignment.Vertical) {",
        "    Row(Modifier.size(121.dp, 65.dp).background(Dim), arrangement, alignment) {",
        "        Block(16, 12, Red); Block(22, 30, Blue); Block(12, 20, Gold)",
        "    }",
        "}",
        "",
        "@Composable",
        "private fun ColumnProbe(arrangement: Arrangement.Vertical, alignment: Alignment.Horizontal) {",
        "    Column(Modifier.size(89.dp, 97.dp).background(Dim), arrangement, alignment) {",
        "        Block(12, 16, Red); Block(30, 22, Blue); Block(20, 12, Gold)",
        "    }",
        "}",
        "",
        "@Composable",
        "private fun BoxProbe(alignment: Alignment) {",
        "    Box(Modifier.size(121.dp, 91.dp).background(Dim), contentAlignment = alignment) {",
        "        Block(30, 20, Red)",
        "    }",
        "}",
        "",
        f"private val BlockColors = listOf({', '.join(kotlin_color(c) for c in BLOCK_COLORS)})",
        "",
        "@Composable",
        "private fun WeightsProbe(children: List<Pair<Float, Boolean>>) {",
        "    Row(Modifier.size(121.dp, 40.dp).background(Dim)) {",
        "        children.forEachIndexed { at, (weight, fill) ->",
        "            val color = BlockColors[at % BlockColors.size]",
        "            if (weight == 0f) {",
        "                Block(18, 24, color)",
        "            } else {",
        "                Box(Modifier.weight(weight, fill).width(10.dp).height(24.dp).background(color))",
        "            }",
        "        }",
        "    }",
        "}",
    ]
    for frame in pages(all_frames):
        out += [
            "",
            "@Composable",
            f"private fun {frame.function}() {{",
            f"    Column(Modifier.padding({grid['origin']}.dp)) {{",
        ]
        for row in rows_of(frame, grid["columns"]):
            out.append("        Row {")
            for cell in row:
                out.append(
                    f"            Box(Modifier.size({grid['cell_width']}.dp, "
                    f"{grid['cell_height']}.dp).clipToBounds()) {{ {cell.kotlin} }}"
                )
            out.append("        }")
        out += ["    }", "}"]
    return "\n".join(out) + "\n"


def rustfmt(source: str) -> str:
    """Formats generated Rust as `cargo fmt` does: the pinned nightly and
    the workspace's rustfmt.toml, so the fmt check accepts the file."""
    pin = (ROOT / "rust-toolchain-nightly.toml").read_text()
    channel = re.search(r'^channel = "(.*)"', pin, re.MULTILINE)
    if channel is None:
        raise SystemExit("rust-toolchain-nightly.toml names no channel")
    formatted = subprocess.run(
        [
            "rustfmt",
            f"+{channel.group(1)}",
            "--edition",
            RUST_EDITION,
            "--config-path",
            str(ROOT / "rustfmt.toml"),
        ],
        input=source,
        capture_output=True,
        text=True,
        check=False,
    )
    if formatted.returncode != 0:
        raise SystemExit(f"rustfmt failed on the generated Rust:\n{formatted.stderr}")
    return formatted.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="fail when a generated file is stale"
    )
    args = parser.parse_args()
    with DESCRIPTION.open("rb") as handle:
        description = tomllib.load(handle)
    all_frames = frames(description)
    outputs = {
        RUST_OUT: rustfmt(rust_source(description, all_frames)),
        KOTLIN_OUT: kotlin_source(description, all_frames),
    }
    stale = [path for path, text in outputs.items() if not path.exists() or path.read_text() != text]
    if args.check:
        for path in stale:
            print(f"{path.relative_to(ROOT)} is stale: run {Path(__file__).relative_to(ROOT)}")
        return 1 if stale else 0
    for path in stale:
        path.write_text(outputs[path])
        print(f"wrote {path.relative_to(ROOT)}")
    cells = sum(len(frame.cells) for frame in all_frames)
    print(f"{len(all_frames)} frames, {cells} cells")
    return 0


if __name__ == "__main__":
    sys.exit(main())
