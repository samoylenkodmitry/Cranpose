#!/usr/bin/env python3
"""Writes the glTF models that the cranpose-xr tests and the xr-demo load.

Each file is made by hand from a few vertices, a PNG that this script encodes and a few keyframes,
so the repository holds no model from elsewhere. Run it from the repository's root:

    python3 scripts/dev/make_gltf_models.py

The tests in crates/cranpose-xr read these files, and their expectations (the bounds of
tower.glb, the colors of the quadrants of checker_quad.gltf) follow from the numbers below.
"""

import base64
import json
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TESTS = ROOT / "crates/cranpose-xr/tests/models"
DEMO = ROOT / "apps/xr-demo/assets"

FLOAT, USHORT = 5126, 5123
ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER = 34962, 34963


def png(width, height, pixel):
    """An 8-bit RGBA PNG whose pixel (x, y) is `pixel(x, y)`."""
    rows = b"".join(
        b"\x00" + b"".join(bytes(pixel(x, y)) for x in range(width)) for y in range(height)
    )

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def quadrants(side):
    """Red top left, green top right, blue bottom left, yellow bottom right."""

    def pixel(x, y):
        left, top = x < side // 2, y < side // 2
        return {
            (True, True): (255, 0, 0, 255),
            (False, True): (0, 255, 0, 255),
            (True, False): (0, 0, 255, 255),
            (False, False): (255, 255, 0, 255),
        }[(left, top)]

    return png(side, side, pixel)


def crate_texture(side):
    """Magenta and lime tiles with a white frame: colors that no other part of the demo has."""

    def pixel(x, y):
        edge = min(x, y, side - 1 - x, side - 1 - y) < side // 16
        if edge:
            return (255, 255, 255, 255)
        tile = ((x * 4) // side + (y * 4) // side) % 2
        return (230, 30, 200, 255) if tile == 0 else (120, 230, 30, 255)

    return png(side, side, pixel)


class Model:
    """A glTF document with one binary buffer."""

    def __init__(self):
        self.data = bytearray()
        self.views, self.accessors = [], []

    def view(self, payload, target=None):
        while len(self.data) % 4:
            self.data.append(0)
        view = {"buffer": 0, "byteOffset": len(self.data), "byteLength": len(payload)}
        if target:
            view["target"] = target
        self.data += payload
        self.views.append(view)
        return len(self.views) - 1

    def accessor(self, values, kind, component, target=None, bounds=False):
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[kind]
        rows = [value if width > 1 else (value,) for value in values]
        fmt = "<%d%s" % (len(rows) * width, "f" if component == FLOAT else "H")
        flat = [v for row in rows for v in row]
        accessor = {
            "bufferView": self.view(struct.pack(fmt, *flat), target),
            "componentType": component,
            "count": len(values),
            "type": kind,
        }
        if bounds:
            columns = list(zip(*rows))
            accessor["min"] = [min(c) for c in columns]
            accessor["max"] = [max(c) for c in columns]
        self.accessors.append(accessor)
        return len(self.accessors) - 1

    def document(self, uri=None, **parts):
        buffer = {"byteLength": len(self.data)}
        if uri is not None:
            buffer["uri"] = uri
        document = {"asset": {"version": "2.0", "generator": "make_gltf_models.py"}}
        document.update(parts)
        document.update(
            buffers=[buffer], bufferViews=self.views, accessors=self.accessors
        )
        return document


def data_uri(kind, payload):
    return "data:%s;base64,%s" % (kind, base64.b64encode(payload).decode())


def glb(document, binary):
    """The binary container: a JSON chunk and a BIN chunk, each padded to four bytes."""
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary = bytes(binary) + b"\x00" * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    return (
        struct.pack("<4sII", b"glTF", 2, total)
        + struct.pack("<I4s", len(text), b"JSON")
        + text
        + struct.pack("<I4s", len(binary), b"BIN\x00")
        + binary
    )


def turn_y(degrees):
    half = math.radians(degrees) / 2
    return [0.0, math.sin(half), 0.0, math.cos(half)]


CUBE_FACES = [
    # normal, then the face's corners from its top left, as seen from outside
    ((0, 0, 1), [(-1, 1, 1), (1, 1, 1), (-1, -1, 1), (1, -1, 1)]),
    ((0, 0, -1), [(1, 1, -1), (-1, 1, -1), (1, -1, -1), (-1, -1, -1)]),
    ((1, 0, 0), [(1, 1, 1), (1, 1, -1), (1, -1, 1), (1, -1, -1)]),
    ((-1, 0, 0), [(-1, 1, -1), (-1, 1, 1), (-1, -1, -1), (-1, -1, 1)]),
    ((0, 1, 0), [(-1, 1, -1), (1, 1, -1), (-1, 1, 1), (1, 1, 1)]),
    ((0, -1, 0), [(-1, -1, 1), (1, -1, 1), (-1, -1, -1), (1, -1, -1)]),
]


def cube(model, half, textured):
    """A cube of side 2 * half with 24 vertices: normals, and the texture on every face."""
    positions, normals, uvs, indices = [], [], [], []
    for normal, corners in CUBE_FACES:
        base = len(positions)
        for (x, y, z), uv in zip(corners, [(0, 0), (1, 0), (0, 1), (1, 1)]):
            positions.append((x * half[0], y * half[1], z * half[2]))
            normals.append(normal)
            uvs.append(uv)
        indices += [base, base + 2, base + 1, base + 1, base + 2, base + 3]
    attributes = {
        "POSITION": model.accessor(positions, "VEC3", FLOAT, ARRAY_BUFFER, bounds=True),
        "NORMAL": model.accessor(normals, "VEC3", FLOAT, ARRAY_BUFFER),
    }
    if textured:
        attributes["TEXCOORD_0"] = model.accessor(uvs, "VEC2", FLOAT, ARRAY_BUFFER)
    return {
        "attributes": attributes,
        "indices": model.accessor(indices, "SCALAR", USHORT, ELEMENT_ARRAY_BUFFER),
    }


def checker_quad():
    """A quad of a meter, facing +z, with a four color texture and no node transform."""
    model = Model()
    half = 0.5
    corners = [(-half, half, 0), (half, half, 0), (-half, -half, 0), (half, -half, 0)]
    primitive = {
        "attributes": {
            "POSITION": model.accessor(corners, "VEC3", FLOAT, ARRAY_BUFFER, bounds=True),
            "NORMAL": model.accessor([(0, 0, 1)] * 4, "VEC3", FLOAT, ARRAY_BUFFER),
            "TEXCOORD_0": model.accessor(
                [(0, 0), (1, 0), (0, 1), (1, 1)], "VEC2", FLOAT, ARRAY_BUFFER
            ),
        },
        "indices": model.accessor([0, 2, 1, 1, 2, 3], "SCALAR", USHORT, ELEMENT_ARRAY_BUFFER),
        "material": 0,
    }
    document = model.document(
        uri=data_uri("application/octet-stream", bytes(model.data)),
        scene=0,
        scenes=[{"nodes": [0]}],
        nodes=[{"name": "quad", "mesh": 0}],
        meshes=[{"primitives": [primitive]}],
        materials=[
            {
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": 0},
                    "baseColorFactor": [1, 1, 1, 1],
                }
            }
        ],
        textures=[{"source": 0}],
        images=[{"uri": data_uri("image/png", quadrants(64)), "mimeType": "image/png"}],
    )
    (TESTS / "checker_quad.gltf").write_text(json.dumps(document, indent=1))


def tower():
    """A parent that turns a quarter about y and lifts one meter, with a flat colored block two
    meters along its x. The block's half sides are 0.25, 1 and 0.5 before the parent turns it.
    Its box at rest is x -0.5 to 0.5, y 0 to 2 and z -2.25 to -1.75. An animation turns the parent
    a quarter more over two seconds and steps the block along x."""
    model = Model()
    primitive = cube(model, (0.25, 1.0, 0.5), textured=False)
    primitive["material"] = 0
    times = model.accessor([0.0, 2.0], "SCALAR", FLOAT, bounds=True)
    turns = model.accessor([turn_y(0), turn_y(90)], "VEC4", FLOAT)
    step_times = model.accessor([0.0, 1.0, 2.0], "SCALAR", FLOAT, bounds=True)
    steps = model.accessor([(2, 0, 0), (3, 0, 0), (4, 0, 0)], "VEC3", FLOAT)
    document = model.document(
        scene=0,
        scenes=[{"nodes": [0]}],
        nodes=[
            {"name": "tower", "translation": [0, 1, 0], "rotation": turn_y(90), "children": [1]},
            {"name": "block", "translation": [2, 0, 0], "mesh": 0},
        ],
        meshes=[{"primitives": [primitive]}],
        materials=[
            {"pbrMetallicRoughness": {"baseColorFactor": [0.5, 0.25, 0.125, 1.0]}}
        ],
        animations=[
            {
                "name": "swing",
                "samplers": [
                    {"input": times, "output": turns, "interpolation": "LINEAR"},
                    {"input": step_times, "output": steps, "interpolation": "STEP"},
                ],
                "channels": [
                    {"sampler": 0, "target": {"node": 0, "path": "rotation"}},
                    {"sampler": 1, "target": {"node": 1, "path": "translation"}},
                ],
            }
        ],
    )
    (TESTS / "tower.glb").write_bytes(glb(document, model.data))


def triangle_external():
    """One triangle whose buffer is the file next to it, with no material and no normals."""
    model = Model()
    primitive = {
        "attributes": {
            "POSITION": model.accessor(
                [(0, 0, 0), (1, 0, 0), (0, 1, 0)], "VEC3", FLOAT, ARRAY_BUFFER, bounds=True
            )
        },
        "indices": model.accessor([0, 1, 2], "SCALAR", USHORT, ELEMENT_ARRAY_BUFFER),
    }
    document = model.document(
        uri="triangle_external.bin",
        scene=0,
        scenes=[{"nodes": [0]}],
        nodes=[{"mesh": 0}],
        meshes=[{"primitives": [primitive]}],
    )
    (TESTS / "triangle_external.gltf").write_text(json.dumps(document, indent=1))
    (TESTS / "triangle_external.bin").write_bytes(model.data)
    document["buffers"][0]["uri"] = "absent.bin"
    (TESTS / "missing_buffer.gltf").write_text(json.dumps(document, indent=1))


def crate():
    """The demo's crate: a cube of 40 cm with a magenta and lime texture that turns about y once
    in six seconds."""
    model = Model()
    primitive = cube(model, (0.2, 0.2, 0.2), textured=True)
    primitive["material"] = 0
    times = model.accessor([0.0, 1.5, 3.0, 4.5, 6.0], "SCALAR", FLOAT, bounds=True)
    turns = model.accessor([turn_y(d) for d in (0, 90, 180, 270, 360)], "VEC4", FLOAT)
    document = model.document(
        scene=0,
        scenes=[{"nodes": [0]}],
        nodes=[{"name": "crate", "mesh": 0}],
        meshes=[{"primitives": [primitive]}],
        materials=[
            {
                "pbrMetallicRoughness": {
                    "baseColorTexture": {"index": 0},
                    "baseColorFactor": [1, 1, 1, 1],
                }
            }
        ],
        textures=[{"source": 0}],
        images=[{"bufferView": None, "mimeType": "image/png"}],
        animations=[
            {
                "name": "spin",
                "samplers": [{"input": times, "output": turns, "interpolation": "LINEAR"}],
                "channels": [{"sampler": 0, "target": {"node": 0, "path": "rotation"}}],
            }
        ],
    )
    document["images"][0]["bufferView"] = model.view(crate_texture(128))
    document["bufferViews"] = model.views
    document["buffers"] = [{"byteLength": len(model.data)}]
    (DEMO / "spinning_crate.glb").write_bytes(glb(document, model.data))


if __name__ == "__main__":
    TESTS.mkdir(parents=True, exist_ok=True)
    checker_quad()
    tower()
    triangle_external()
    crate()
