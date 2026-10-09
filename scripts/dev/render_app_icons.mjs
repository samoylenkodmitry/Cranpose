// Renders the demo's app icon into the raster files each platform packages.
//
// The one drawing is apps/desktop-demo/assets/icon/icon.svg. The web page
// serves it as it is; every other platform takes pictures of it, which this
// script draws through headless Chrome at each exact pixel size and writes in
// place. Run it after any change to the SVG and commit what it writes:
//
//     node scripts/dev/render_app_icons.mjs
//
// CHROME names another Chrome or Chromium binary.

import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { crc32, deflateSync } from "node:zlib";
import { launchChrome } from "../a11y/chrome.mjs";

const WORKSPACE = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SOURCE = "apps/desktop-demo/assets/icon/icon.svg";
const ANDROID_RES = "apps/android-demo/android/app/src/main/res";
const IOS_BUNDLE = "apps/ios-demo/ios/CranposeDemo";
const MACOS_ICNS = "apps/desktop-demo/assets/icon/CranposeDemo.icns";

// Each variant is the source drawing with a few attributes changed, so every
// platform shows the same picture. It runs in the page on the parsed SVG.
const VARIANTS = {
    // The rounded tile as drawn: the window and legacy launcher icons.
    tile: () => {},
    // iOS rounds the corners itself and refuses transparency.
    square: svg => {
        for (const rect of svg.querySelectorAll("rect")) rect.setAttribute("rx", "0");
    },
    // macOS icons carry their own outline and shadow inside a margin: the
    // tile is 824 of 1024 pixels.
    macos: (svg, ns) => {
        const shadow = svg.ownerDocument.createElementNS(ns, "filter");
        shadow.id = "macos-shadow";
        shadow.setAttribute("y", "-10%");
        shadow.setAttribute("height", "130%");
        shadow.innerHTML = '<feDropShadow dy="12" stdDeviation="14" flood-opacity=".35"/>';
        svg.querySelector("defs").append(shadow);
        svg.querySelector("#backdrop").setAttribute("filter", "url(#macos-shadow)");
        wrap(svg, ns, "translate(100 100) scale(0.8046875)");
    },
    // Android adaptive layers. The launcher cuts its own shape from them.
    background: (svg, ns) => {
        adaptive(svg, ns);
        svg.querySelector("#mark").remove();
    },
    foreground: (svg, ns) => {
        adaptive(svg, ns);
        svg.querySelector("#backdrop").remove();
    },
    // Android 13 themed icons take only the alpha of this layer. The mark is
    // drawn white with black outlines, and the luminance becomes the alpha,
    // so the outlines become gaps that keep the parts apart.
    monochrome: (svg, ns) => {
        adaptive(svg, ns);
        svg.querySelector("#backdrop").remove();
        const mark = svg.querySelector("#mark");
        for (const shape of [mark, ...mark.querySelectorAll("*")]) {
            if (shape.hasAttribute("fill") && shape.getAttribute("fill") !== "none") shape.setAttribute("fill", "#fff");
            if (shape.hasAttribute("stroke") && shape.getAttribute("stroke") !== "none") shape.setAttribute("stroke", "#000");
        }
    },
};

// Page functions the variants call. An Android adaptive layer is 108 dp of
// which the launcher shows the middle 72 dp, so the tile becomes those 72 dp
// and the backdrop runs on to the layer's edges.
const PAGE_HELPERS = `
function wrap(svg, ns, transform) {
    const group = svg.ownerDocument.createElementNS(ns, "g");
    group.setAttribute("transform", transform);
    group.append(...[...svg.children].filter(child => child.localName !== "defs"));
    svg.append(group);
}
function adaptive(svg, ns) {
    for (const rect of svg.querySelectorAll("#backdrop rect")) {
        rect.setAttribute("x", "-256");
        rect.setAttribute("y", "-256");
        rect.setAttribute("width", "1536");
        rect.setAttribute("height", "1536");
    }
    for (const rect of svg.querySelectorAll("rect")) rect.setAttribute("rx", "0");
    wrap(svg, ns, "translate(512 512) scale(0.6666667) translate(-512 -512)");
}
async function renderIcon(source, variant, size) {
    const ns = "http://www.w3.org/2000/svg";
    const svg = new DOMParser().parseFromString(source, "image/svg+xml").documentElement;
    variant(svg, ns);
    const url = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(svg)], { type: "image/svg+xml" }));
    try {
        const image = new Image(size, size);
        image.src = url;
        await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const context = canvas.getContext("2d");
        context.drawImage(image, 0, 0, size, size);
        const pixels = context.getImageData(0, 0, size, size).data;
        let binary = "";
        for (let index = 0; index < pixels.length; index += 0x8000) {
            binary += String.fromCharCode(...pixels.subarray(index, index + 0x8000));
        }
        return btoa(binary);
    } finally {
        URL.revokeObjectURL(url);
    }
}
`;

// Draws `source` once per request ({ variant, size }) and returns straight
// RGBA pixels, `size` by `size`.
export async function renderPixels(source, requests) {
    const logs = mkdtempSync(join(tmpdir(), "app-icons-"));
    const { cdp, close } = await launchChrome([], logs);
    try {
        const variants = Object.entries(VARIANTS)
            .map(([name, apply]) => `${JSON.stringify(name)}: ${apply.toString()}`)
            .join(",\n");
        await cdp.evaluate(`${PAGE_HELPERS}; window.variants = {${variants}}; window.renderIcon = renderIcon; true`);
        const images = [];
        for (const { variant, size } of requests) {
            if (!(variant in VARIANTS)) throw new Error(`unknown icon variant ${variant}`);
            const base64 = await cdp.evaluate(
                `renderIcon(${JSON.stringify(source)}, variants[${JSON.stringify(variant)}], ${size})`,
            );
            const rgba = Buffer.from(base64, "base64");
            if (variant === "monochrome") luminanceToAlpha(rgba);
            images.push(rgba);
        }
        return images;
    } finally {
        await close();
        rmSync(logs, { recursive: true, force: true });
    }
}

function luminanceToAlpha(rgba) {
    for (let index = 0; index < rgba.length; index += 4) {
        const luminance = (rgba[index] + rgba[index + 1] + rgba[index + 2]) / 765;
        rgba[index + 3] = Math.round(rgba[index + 3] * luminance);
        rgba.fill(255, index, index + 3);
    }
}

function paeth(left, up, upLeft) {
    const estimate = left + up - upLeft;
    const toLeft = Math.abs(estimate - left);
    const toUp = Math.abs(estimate - up);
    const toUpLeft = Math.abs(estimate - upLeft);
    if (toLeft <= toUp && toLeft <= toUpLeft) return left;
    return toUp <= toUpLeft ? up : upLeft;
}

// Applies PNG filter `filter` to `row` against the row above it, writes the
// residuals to `out` when given, and returns their total size.
function filterRow(filter, row, previous, channels, out) {
    let cost = 0;
    for (let index = 0; index < row.length; index++) {
        const left = index >= channels ? row[index - channels] : 0;
        const up = previous[index];
        let predicted = 0;
        if (filter === 1) predicted = left;
        else if (filter === 2) predicted = up;
        else if (filter === 3) predicted = (left + up) >> 1;
        else if (filter === 4) predicted = paeth(left, up, index >= channels ? previous[index - channels] : 0);
        const residual = (row[index] - predicted) & 255;
        if (out) out[index] = residual;
        cost += residual < 128 ? residual : 256 - residual;
    }
    return cost;
}

// A PNG of square RGBA pixels, RGB when `opaque`. Each row takes the filter
// with the smallest residuals, and zlib packs the rows at its highest level:
// Chrome's own encoder writes files about half again as large.
export function encodePng(rgba, size, opaque) {
    const channels = opaque ? 3 : 4;
    const stride = size * channels;
    const rows = Buffer.alloc((stride + 1) * size);
    let previous = Buffer.alloc(stride);
    let row = Buffer.alloc(stride);
    for (let y = 0; y < size; y++) {
        for (let x = 0; x < size; x++) {
            const pixel = (y * size + x) * 4;
            if (opaque && rgba[pixel + 3] !== 255) throw new Error("an opaque icon has a transparent pixel");
            rgba.copy(row, x * channels, pixel, pixel + channels);
        }
        let best = 0;
        let bestCost = Infinity;
        for (let filter = 0; filter < 5; filter++) {
            const cost = filterRow(filter, row, previous, channels, null);
            if (cost < bestCost) [best, bestCost] = [filter, cost];
        }
        const start = y * (stride + 1);
        rows[start] = best;
        filterRow(best, row, previous, channels, rows.subarray(start + 1, start + 1 + stride));
        [previous, row] = [row, previous];
    }
    const header = Buffer.alloc(13);
    header.writeUInt32BE(size, 0);
    header.writeUInt32BE(size, 4);
    header.writeUInt8(8, 8);
    header.writeUInt8(opaque ? 2 : 6, 9);
    return Buffer.concat([
        Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
        pngChunk("IHDR", header),
        pngChunk("IDAT", deflateSync(rows, { level: 9, memLevel: 9 })),
        pngChunk("IEND", Buffer.alloc(0)),
    ]);
}

function pngChunk(type, data) {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const typed = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(crc32(typed));
    return Buffer.concat([length, typed, crc]);
}

// An .icns file is its header followed by (type, length, PNG) entries.
function icns(entries) {
    const header = (type, length) => {
        const bytes = Buffer.alloc(8);
        bytes.write(type, 0, "ascii");
        bytes.writeUInt32BE(length, 4);
        return bytes;
    };
    const chunks = entries.map(({ type, png }) => Buffer.concat([header(type, png.length + 8), png]));
    const length = chunks.reduce((total, chunk) => total + chunk.length, 8);
    return Buffer.concat([header("icns", length), ...chunks]);
}

// Every picture this script writes: the variant, its pixel size, and whether
// the platform refuses transparency.
function pictures() {
    const files = [
        { path: "apps/desktop-demo/assets/icon/window-icon.png", variant: "tile", size: 128 },
        // The names iOS looks up from CFBundleIconFiles in Info.plist.
        { path: `${IOS_BUNDLE}/AppIcon60x60@2x.png`, variant: "square", size: 120, opaque: true },
        { path: `${IOS_BUNDLE}/AppIcon60x60@3x.png`, variant: "square", size: 180, opaque: true },
        { path: `${IOS_BUNDLE}/AppIcon76x76@2x~ipad.png`, variant: "square", size: 152, opaque: true },
        { path: `${IOS_BUNDLE}/AppIcon83.5x83.5@2x~ipad.png`, variant: "square", size: 167, opaque: true },
        // The background holds only soft glows, so one size scaled to every
        // density looks the same as one per density.
        { path: `${ANDROID_RES}/drawable-nodpi/ic_launcher_background.png`, variant: "background", size: 216 },
    ];
    const densities = { mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 };
    for (const [density, scale] of Object.entries(densities)) {
        const folder = `${ANDROID_RES}/mipmap-${density}`;
        // Android 7 has no adaptive icons and shows this one.
        files.push({ path: `${folder}/ic_launcher.png`, variant: "tile", size: 48 * scale });
        files.push({ path: `${folder}/ic_launcher_foreground.png`, variant: "foreground", size: 108 * scale });
        files.push({ path: `${folder}/ic_launcher_monochrome.png`, variant: "monochrome", size: 108 * scale });
    }
    return files;
}

// The .icns entry type for each pixel size, one entry per size: macOS picks a
// picture by its pixels, so 64 pixels serves as 32 points at twice the
// density. Past 512 pixels the file grows fourfold for a size only a
// full-screen Quick Look draws.
const ICNS_TYPES = { 16: "icp4", 32: "icp5", 64: "ic12", 128: "ic07", 256: "ic08", 512: "ic09" };

function write(path, bytes) {
    mkdirSync(dirname(join(WORKSPACE, path)), { recursive: true });
    writeFileSync(join(WORKSPACE, path), bytes);
    console.log(`${path} (${bytes.length} bytes)`);
}

async function main() {
    const source = readFileSync(join(WORKSPACE, SOURCE), "utf8");
    const files = pictures();
    const icnsSizes = Object.keys(ICNS_TYPES).map(Number);
    const images = await renderPixels(source, [...files, ...icnsSizes.map(size => ({ variant: "macos", size }))]);
    files.forEach(({ path, size, opaque }, index) => write(path, encodePng(images[index], size, opaque === true)));
    const entries = icnsSizes.map((size, index) => ({
        type: ICNS_TYPES[size],
        png: encodePng(images[files.length + index], size, false),
    }));
    write(MACOS_ICNS, icns(entries));
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
    await main();
}
