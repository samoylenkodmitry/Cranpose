#!/usr/bin/env node
// Opens the packaged web demo on one GPU backend, visits every page it can
// start on and fails on any GPU error, panic or blank frame.
// - webgpu: Chrome's WGSL compiler (Tint) rejects shaders that naga accepts,
//   and only a real WebGPU device catches those.
// - gl: WebGL2, which phones without WebGPU use. The GL backend reads a
//   program's link at its first draw, so a program that fails to link shows
//   only there.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { launchChrome } from "../chrome.mjs";

const url = process.argv[2];
const output = process.argv[3];
const backend = process.argv[4] ?? "webgpu";
assert.ok(url && output, "pass the URL of the packaged web demo and an output directory");

// What each backend selects, how the renderer names it and what fails it.
// Chrome reports WebGPU validation as console warnings ("Error while parsing
// WGSL", "[Invalid RenderPipeline ...]"); the renderer logs the same errors
// and a lost device under [gpu-device].
const BACKENDS = {
    webgpu: { name: "WebGPU", selected: "BrowserWebGpu", failure: /WGSL|\[Invalid \w+|WebGPU/ },
    gl: { name: "WebGL2", selected: "Gl", failure: /did not link|GL_INVALID|WebGL: / },
};
const BACKEND = BACKENDS[backend];
assert.ok(BACKEND, `the backend is one of ${Object.keys(BACKENDS).join(", ")}`);
const FAILURE = new RegExp(
    `\\[gpu-device\\]|panicked|exception:|Failed to initialize app|${BACKEND.failure.source}`);
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
    page.searchParams.set("backend", backend);
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
    if (!lines.some(line => line.includes(`selected backend=${BACKEND.selected},`))) {
        errors.push(`the page did not select ${BACKEND.name}: ` + (lines.find(line => line.includes("selected backend=")) ?? "no backend line"));
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

// The page's device lost on purpose (`cranpose_lose_gpu_device`): the page
// opens another and draws on. The renderer's own [gpu-device] lines about the
// loss and its recovery are expected here, and so is WebGL's note that an
// object freed after the restore belonged to the lost context, which WebGL
// then ignores; any other failure line is not.
const EXPECTED_LOSS = /\[gpu-device\] (losing the device|device lost|the WebGL context (was lost|is back)|opened a new device)|WebGL: INVALID_OPERATION: \w+: object does not belong to this context/;
const RECOVERED = "[gpu-device] opened a new device after the last one was lost";
// The drill loses the device at the first frame two seconds after start.
const LOSS_WAIT_MS = 20000;

async function deviceLoss() {
    const page = new URL(url);
    page.searchParams.set("backend", backend);
    page.searchParams.set("cranpose_lose_gpu_device", "1");
    const start = cdp.logs.length;
    await cdp.send("Page.navigate", { url: page.href });
    await cdp.until(`location.href === ${JSON.stringify(page.href)}
        && ['running', 'error'].includes(window.__cranposeAppState?.phase)`);
    const errors = [];
    const deadline = Date.now() + LOSS_WAIT_MS;
    // Pointer moves over the canvas keep frames coming for the drill.
    for (let step = 0; Date.now() < deadline; step++) {
        if (cdp.logs.slice(start).some(line => line.includes(RECOVERED))) break;
        await cdp.send("Input.dispatchMouseEvent", {
            type: "mouseMoved", x: 200 + (step % 40) * 10, y: 300 + (step % 7) * 20,
        });
        await pause(100);
    }
    if (!cdp.logs.slice(start).some(line => line.includes(RECOVERED))) {
        errors.push("the page did not open a new device after losing its device");
    }
    await pause(SETTLE_MS);
    const phase = await cdp.evaluate("window.__cranposeAppState.phase");
    if (phase !== "running") errors.push(`the page stopped in phase ${phase}`);
    const colors = await distinctColors("device-loss.png");
    if (colors < MIN_COLORS) errors.push(`after the recovery the page drew ${colors} distinct colors`);
    await cdp.send("Page.navigate", { url: "about:blank" });
    await cdp.until("location.href === 'about:blank'");
    await pause(LEAVE_MS);
    const lines = cdp.logs.slice(start);
    errors.push(...lines.filter(line => FAILURE.test(line) && !EXPECTED_LOSS.test(line)));
    results.push({ page: "(device loss)", colors, errors, console: lines });
    console.log(`${errors.length ? "FAIL" : "ok  "} (device loss) colors=${colors}`);
    for (const error of new Set(errors.map(error => error.split("\n")[0]))) console.log("     " + error);
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
    if (backend === "webgpu") {
        const adapter = await cdp.evaluate(`(async () => {
            const adapter = await navigator.gpu?.requestAdapter();
            return adapter ? [adapter.info.vendor, adapter.info.architecture, adapter.info.isFallbackAdapter ? '(fallback)' : ''].join(' ') : '';
        })()`);
        assert.ok(adapter, "Chrome offers no WebGPU adapter here, so this check cannot run");
        console.log(`WebGPU adapter: ${adapter}`);
    }
    // The build under test names its own pages, so a new tab is visited
    // without a change here.
    const pages = await visit("", 0, () => cdp.evaluate(`(async () => {
        const manifest = await fetch('asset-manifest.json', { cache: 'no-store' }).then(response => response.json());
        const demo = await import(new URL(manifest.module, document.baseURI).href);
        return demo.demo_startup_pages();
    })()`));
    assert.ok(pages, "the default page did not start, so the demo's page list is unknown");
    for (const [index, query] of pages.entries()) await visit(query, index + 1);
    await deviceLoss();
    const failed = results.filter(result => result.errors.length);
    printErrors();
    console.log(`${results.length - failed.length} of ${results.length} pages passed on ${BACKEND.name}`);
    status = failed.length ? 1 : 0;
} finally {
    writeFileSync(join(output, "report.json"), JSON.stringify({ platform: backend, pages: results }, null, 2));
    await close();
}
process.exit(status);
