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


def half_clear(side):
    """Transparent on the left half, opaque green on the right half."""
    return png(side, side, lambda x, y: (0, 255, 0, 255) if x >= side // 2 else (255, 0, 0, 0))


def crate_texture(side):
    """Magenta and lime tiles with a white frame. The `model` build of the xr-demo shows only the crate,
    so its screenshots hold these hues alone. The full demo's test patterns have them too."""

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
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[kind]
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


def textured_quad(name, texture, material, textures=None, images=None, **extras):
    """A quad of a meter, facing +z, with a texture and no node transform. The PNG `texture` is the
    image of the only texture, unless `textures` and `images` say otherwise."""
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
        materials=[material],
        textures=textures or [{"source": 0}],
        images=images or [{"uri": data_uri("image/png", texture), "mimeType": "image/png"}],
        **extras,
    )
    (TESTS / name).write_text(json.dumps(document, indent=1))


def checker_quad():
    """A quad with a four color texture of 512 pixels, more than the windows of the tests."""
    textured_quad(
        "checker_quad.gltf",
        quadrants(512),
        {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 1]}},
    )


def masked_quad():
    """A quad whose texture is clear on the left half, in the MASK alpha mode."""
    textured_quad(
        "masked_quad.gltf",
        half_clear(64),
        {
            "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}},
            "alphaMode": "MASK",
            "alphaCutoff": 0.5,
        },
    )


def ktx2_solid(color):
    """A KTX2 file with one UASTC block: four by four pixels of the RGBA `color`, in the block's
    solid color mode. The mode's 5 bit code comes first, then the four channels of 8 bits each, from
    the lowest bit up."""
    block = 0x17 | sum(channel << (5 + 8 * index) for index, channel in enumerate(color))
    # The data format descriptor of a UASTC texture: color model 166, primaries BT.709, the sRGB
    # transfer function, blocks of 4 by 4 texels and 16 bytes, and one sample of 128 bits
    descriptor = (
        struct.pack("<IIHH", 44, 0, 2, 40)
        + bytes([166, 1, 2, 0, 3, 3, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0])
        + struct.pack("<HBBBBBBII", 0, 127, 0, 0, 0, 0, 0, 0, 0xFFFFFFFF)
    )
    identifier = b"\xabKTX 20\xbb\r\n\x1a\n"
    # No supercompression, one level, one face. The descriptor starts after the 24 bytes of the
    # level index, and the level starts at the next multiple of 16.
    header = struct.pack("<9I", 0, 1, 4, 4, 0, 0, 1, 1, 0)
    index = struct.pack("<4I2Q", 104, len(descriptor), 0, 0, 0, 0)
    level = struct.pack("<3Q", 160, 16, 16)
    head = identifier + header + index + level + descriptor
    return head + bytes(160 - len(head)) + block.to_bytes(16, "little")


def basisu_quad():
    """A quad whose only texture names a KTX2 image through KHR_texture_basisu and has no source of
    its own, as the controller models of a Meta Quest do."""
    textured_quad(
        "basisu_quad.gltf",
        None,
        {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 1]}},
        textures=[{"extensions": {"KHR_texture_basisu": {"source": 0}}}],
        images=[{"uri": data_uri("image/ktx2", ktx2_solid((200, 100, 50, 255))), "mimeType": "image/ktx2"}],
        extensionsUsed=["KHR_texture_basisu"],
        extensionsRequired=["KHR_texture_basisu"],
    )


def basisu_fallback_quad():
    """The same quad, whose texture also has a PNG as its source, which a reader that lacks the
    extension shows. The PNG is the four colors of checker_quad.gltf."""
    textured_quad(
        "basisu_fallback_quad.gltf",
        None,
        {"pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 1]}},
        textures=[{"source": 1, "extensions": {"KHR_texture_basisu": {"source": 0}}}],
        images=[
            {"uri": data_uri("image/ktx2", ktx2_solid((200, 100, 50, 255))), "mimeType": "image/ktx2"},
            {"uri": data_uri("image/png", quadrants(64)), "mimeType": "image/png"},
        ],
        extensionsUsed=["KHR_texture_basisu"],
    )


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


def turn(axis, degrees):
    """The quaternion of a turn by `degrees` about `axis`."""
    half = math.radians(degrees) / 2
    length = math.sqrt(sum(a * a for a in axis))
    return [a / length * math.sin(half) for a in axis] + [math.cos(half)]


# The parts of the test controller: the node's name, where it stands, the path that its keys move,
# and the pose at each key that differs from the rest. The keys are the ones of a Meta Quest's
# runtime models (crates/cranpose-xr/src/controller_parts.rs). A second key after each press holds a
# smaller pose, so a reader that takes the wrong key, or a time, finds a wrong pose.
STICK_TILT = 15
CONTROLLER_PARTS = [
    ("b_trigger_front", (0.0, -0.02, -0.04), "rotation",
     {16: turn((1, 0, 0), 20), 17: turn((1, 0, 0), 7)}),
    ("b_trigger_grip", (0.02, -0.05, 0.0), "rotation",
     {21: turn((0, 0, 1), -12), 22: turn((0, 0, 1), -4)}),
    ("b_button_a", (0.01, 0.02, 0.0), "translation",
     {5: (0.01, 0.018, 0.0), 6: (0.01, 0.019, 0.0)}),
    ("b_button_b", (-0.01, 0.02, 0.0), "translation",
     {8: (-0.01, 0.018, 0.0), 9: (-0.01, 0.019, 0.0)}),
    ("b_button_oculus", (0.0, 0.02, 0.02), "translation", {24: (0.0, 0.019, 0.02)}),
    # The cap leans toward -z for forward and toward +x for the right: the turn's axis is up
    # crossed with the lean
    ("b_thumbstick", (0.0, 0.02, -0.02), "rotation", {
        key: turn((dz, 0, -dx), STICK_TILT)
        for key, (dx, dz) in {
            29: (0, -1), 39: (1, -1), 34: (1, 0), 40: (1, 1),
            31: (0, 1), 36: (-1, 1), 32: (-1, 0), 37: (-1, -1),
        }.items()
    } | {30: turn((1, 0, 0), 3)}),
]
CONTROLLER_KEYS = 42


def controller(name, blended):
    """A test controller in the layout of a Meta Quest's runtime models: a body and a cube of 1 cm
    for each part, in one skinned mesh, and one animation whose fixed keys hold the parts' poses.
    The trigger is a lever of 3 cm that hangs from its joint, so its turn swings its end by 1 cm.
    Every vertex follows one joint, unless `blended`: then one corner of the trigger's cube weighs
    the trigger and the body half each. The body's box is x -0.02 to 0.02, y -0.01 to 0.01 and z
    -0.05 to 0.05. With the parts, the model's box at rest is x -0.02 to 0.025, y -0.055 to 0.025 and
    z -0.05 to 0.05."""
    model = Model()
    positions, normals, joints, weights, indices = [], [], [], [], []

    def block(center, half, joint):
        for normal, corners in CUBE_FACES:
            base = len(positions)
            for x, y, z in corners:
                positions.append(
                    (center[0] + x * half[0], center[1] + y * half[1], center[2] + z * half[2])
                )
                normals.append(normal)
                joints.append((joint, 0, 0, 0))
                weights.append((1.0, 0.0, 0.0, 0.0))
            indices.extend([base, base + 2, base + 1, base + 1, base + 2, base + 3])

    block((0, 0, 0), (0.02, 0.01, 0.05), 0)
    for joint, (part, at, _, _) in enumerate(CONTROLLER_PARTS, start=1):
        if part == "b_trigger_front":
            block((at[0], at[1] - 0.015, at[2]), (0.005, 0.015, 0.005), joint)
        else:
            block(at, (0.005, 0.005, 0.005), joint)
    if blended:
        trigger_corner = 24
        joints[trigger_corner] = (1, 0, 0, 0)
        weights[trigger_corner] = (0.5, 0.5, 0.0, 0.0)
    primitive = {
        "attributes": {
            "POSITION": model.accessor(positions, "VEC3", FLOAT, ARRAY_BUFFER, bounds=True),
            "NORMAL": model.accessor(normals, "VEC3", FLOAT, ARRAY_BUFFER),
            "JOINTS_0": model.accessor(joints, "VEC4", USHORT, ARRAY_BUFFER),
            "WEIGHTS_0": model.accessor(weights, "VEC4", FLOAT, ARRAY_BUFFER),
        },
        "indices": model.accessor(indices, "SCALAR", USHORT, ELEMENT_ARRAY_BUFFER),
        "material": 0,
    }
    # The joints stand at their parts without a turn, so each inverse bind matrix moves back by
    # the joint's place: four columns, the last one the translation
    unbind = [
        (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1)
        for x, y, z in [(0, 0, 0)] + [at for _, at, _, _ in CONTROLLER_PARTS]
    ]
    times = model.accessor(
        [key / 24 for key in range(CONTROLLER_KEYS)], "SCALAR", FLOAT, bounds=True
    )
    samplers, channels = [], []
    for node, (_, at, path, poses) in enumerate(CONTROLLER_PARTS, start=2):
        rest = list(at) if path == "translation" else [0.0, 0.0, 0.0, 1.0]
        values = [poses.get(key, rest) for key in range(CONTROLLER_KEYS)]
        kind = "VEC3" if path == "translation" else "VEC4"
        output = model.accessor(values, kind, FLOAT)
        samplers.append({"input": times, "output": output, "interpolation": "LINEAR"})
        channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path}})
    document = model.document(
        scene=0,
        scenes=[{"nodes": [0]}],
        nodes=[
            {"name": "root", "children": [1, 8]},
            {"name": "body", "children": [2, 3, 4, 5, 6, 7]},
        ]
        + [{"name": part, "translation": list(at)} for part, at, _, _ in CONTROLLER_PARTS]
        + [{"name": "controller_mesh", "mesh": 0, "skin": 0}],
        meshes=[{"primitives": [primitive]}],
        materials=[{"pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.8, 0.8, 1.0]}}],
        skins=[
            {
                "joints": [1, 2, 3, 4, 5, 6, 7],
                "inverseBindMatrices": model.accessor(unbind, "MAT4", FLOAT),
            }
        ],
        animations=[{"name": "All Animations", "samplers": samplers, "channels": channels}],
    )
    (TESTS / name).write_bytes(glb(document, model.data))


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
    masked_quad()
    basisu_quad()
    basisu_fallback_quad()
    tower()
    controller("controller_parts.glb", blended=False)
    controller("controller_blended.glb", blended=True)
    triangle_external()
    crate()
