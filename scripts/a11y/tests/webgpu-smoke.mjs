#!/usr/bin/env node
// Opens the packaged web demo on WebGPU, visits every page it can start on and
// fails on any GPU validation error, panic or blank frame. Chrome's WGSL
// compiler (Tint) rejects shaders that naga accepts, and the WebGL robots never
// compile WGSL in the browser, so only a real WebGPU device catches those.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { launchChrome } from "../chrome.mjs";

const url = process.argv[2];
const output = process.argv[3];
assert.ok(url && output, "pass the URL of the packaged web demo and an output directory");

// Chrome reports WebGPU validation as console warnings ("Error while parsing
// WGSL", "[Invalid RenderPipeline ...]"); the renderer logs the same errors
// and a lost device under [gpu-device].
const FAILURE = /\[gpu-device\]|WGSL|\[Invalid \w+|WebGPU|panicked|exception:|Failed to initialize app/;
// Headless Chrome on Linux offers WebGPU only behind these switches: the GPU
// through Vulkan, or SwiftShader where there is none. macOS uses Metal.
const PLATFORM_FLAGS = process.platform === "linux"
    ? ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-angle=vulkan"]
    : [];
const WIDTH = 1280;
const HEIGHT = 1600;
// Uncaptured GPU errors arrive asynchronously, a few frames after the bad call.
const SETTLE_MS = 1500;
// Chrome delivers a page's last GPU warnings after the next navigation starts.
const LEAVE_MS = 500;
// A page that failed to draw shows one flat color.
const MIN_COLORS = 8;

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const { cdp, close } = await launchChrome([`--window-size=${WIDTH},${HEIGHT}`, ...PLATFORM_FLAGS], output);
const results = [];

async function distinctColors(file) {
    const { data } = await cdp.send("Page.captureScreenshot", {
        format: "png",
        clip: { x: 0, y: 0, width: WIDTH, height: HEIGHT, scale: 0.25 },
    });
    writeFileSync(join(output, file), Buffer.from(data, "base64"));
    return cdp.evaluate(`(async () => {
        const image = new Image();
        image.src = 'data:image/png;base64,${data}';
        await image.decode();
        const context = new OffscreenCanvas(image.width, image.height).getContext('2d');
        context.drawImage(image, 0, 0);
        const pixels = context.getImageData(0, 0, image.width, image.height).data;
        return new Set(new Uint32Array(pixels.buffer)).size;
    })()`);
}

// Opens one page, runs `whileOpen` on it and returns what that returns.
async function visit(query, index, whileOpen = async () => undefined) {
    const page = new URL(url);
    page.search = query;
    page.searchParams.set("backend", "webgpu");
    const start = cdp.logs.length;
    await cdp.send("Page.navigate", { url: page.href });
    await cdp.until(`location.href === ${JSON.stringify(page.href)}
        && ['running', 'error'].includes(window.__cranposeAppState?.phase)`);
    await pause(SETTLE_MS);
    const phase = await cdp.evaluate("window.__cranposeAppState.phase");
    const colors = await distinctColors(`page-${String(index).padStart(2, "0")}.png`);
    const found = phase === "running" ? await whileOpen() : undefined;
    await cdp.send("Page.navigate", { url: "about:blank" });
    await cdp.until("location.href === 'about:blank'");
    await pause(LEAVE_MS);
    const lines = cdp.logs.slice(start);
    const errors = lines.filter(line => FAILURE.test(line));
    if (phase !== "running") errors.push(`the page stopped in phase ${phase}`);
    if (!lines.some(line => line.includes("selected backend=BrowserWebGpu"))) {
        errors.push("the page did not select WebGPU: " + (lines.find(line => line.includes("selected backend=")) ?? "no backend line"));
    }
    for (const [param, override] of [["tab", "tab"], ["shader_section", "shader section"]]) {
        if (page.searchParams.has(param) && !lines.some(line => line.includes(`Applying startup ${override} override`))) {
            errors.push(`the page did not open its ${override}`);
        }
    }
    if (colors < MIN_COLORS) errors.push(`the page drew ${colors} distinct colors`);
    results.push({ page: query || "(default)", colors, errors, console: lines });
    console.log(`${errors.length ? "FAIL" : "ok  "} ${query || "(default)"} colors=${colors}`);
    for (const error of new Set(errors.map(error => error.split("\n")[0]))) console.log("     " + error);
    return found;
}

// Prints each distinct error once in full, with the pages that showed it.
function printErrors() {
    const pages = new Map();
    for (const result of results) {
        for (const error of new Set(result.errors)) pages.set(error, [...(pages.get(error) ?? []), result.page]);
    }
    for (const [error, where] of pages) console.log(`\n--- on ${where.length} page(s): ${where.join(", ")}\n${error}`);
}

let status = 1;
try {
    // Any page of the site is a secure context that may ask for an adapter.
    const manifest = new URL("asset-manifest.json", url).href;
    await cdp.send("Page.navigate", { url: manifest });
    await cdp.until(`location.href === ${JSON.stringify(manifest)} && document.readyState === 'complete'`);
    const adapter = await cdp.evaluate(`(async () => {
        const adapter = await navigator.gpu?.requestAdapter();
        return adapter ? [adapter.info.vendor, adapter.info.architecture, adapter.info.isFallbackAdapter ? '(fallback)' : ''].join(' ') : '';
    })()`);
    assert.ok(adapter, "Chrome offers no WebGPU adapter here, so this check cannot run");
    console.log(`WebGPU adapter: ${adapter}`);
    // The build under test names its own pages, so a new tab is visited
    // without a change here.
    const pages = await visit("", 0, () => cdp.evaluate(`(async () => {
        const manifest = await fetch('asset-manifest.json', { cache: 'no-store' }).then(response => response.json());
        const demo = await import(new URL(manifest.module, document.baseURI).href);
        return demo.demo_startup_pages();
    })()`));
    assert.ok(pages, "the default page did not start, so the demo's page list is unknown");
    for (const [index, query] of pages.entries()) await visit(query, index + 1);
    const failed = results.filter(result => result.errors.length);
    printErrors();
    console.log(`${results.length - failed.length} of ${results.length} pages passed on WebGPU`);
    status = failed.length ? 1 : 0;
} finally {
    writeFileSync(join(output, "report.json"), JSON.stringify({ platform: "webgpu", pages: results }, null, 2));
    await close();
}
process.exit(status);
