#!/usr/bin/env node
// Opens the packaged web demo on one GPU backend, visits every page it can
// start on and fails on any GPU error, panic or blank frame.
// - webgpu: Chrome's WGSL compiler (Tint) rejects shaders that naga accepts,
//   and only a real WebGPU device catches those.
// - gl: WebGL2, which phones without WebGPU use. The GL backend reads a
//   program's link at its first draw, so a program that fails to link shows
//   only there. A release draws its first screen with the exact GLSL beside
//   its wasm, and a page that cannot take that file says so. Its frames go
//   straight into the canvas, upside down, and CSS flips the canvas: the
//   default page must look as it does on WebGPU, where Chrome offers that.
// On both, a press where the page shows a button must reach that button.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { ENABLE_MIRROR } from "../cdp.mjs";
import { GPU_FLAGS, launchChrome } from "../chrome.mjs";

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
    gl: { name: "WebGL2", selected: "Gl", failure: /did not link|GL_INVALID|WebGL: |\[first-screen-glsl\]/ },
};
const BACKEND = BACKENDS[backend];
assert.ok(BACKEND, `the backend is one of ${Object.keys(BACKENDS).join(", ")}`);
const FAILURE = new RegExp(
    `\\[gpu-device\\]|panicked|exception:|Failed to initialize app|${BACKEND.failure.source}`);
const WIDTH = 1280;
const HEIGHT = 1600;
// Uncaptured GPU errors arrive asynchronously, a few frames after the bad call.
const SETTLE_MS = 1500;
// Chrome delivers a page's last GPU warnings after the next navigation starts.
const LEAVE_MS = 500;
// A page that failed to draw shows one flat color.
const MIN_COLORS = 8;
// How long a page that has not drawn yet may still take: a first frame waits
// for its pipelines, and on a loaded runner a cold glass compile can outlast
// the settle (the guide page on an Intel gen-12lp runner).
const FIRST_DRAW_WAIT_MS = 8000;

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const { cdp, close } = await launchChrome([`--window-size=${WIDTH},${HEIGHT}`, ...GPU_FLAGS], output);
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
    const shot = `page-${String(index).padStart(2, "0")}.png`;
    let colors = await distinctColors(shot);
    const drawDeadline = Date.now() + FIRST_DRAW_WAIT_MS;
    while (phase === "running" && colors < MIN_COLORS && Date.now() < drawDeadline) {
        await pause(250);
        colors = await distinctColors(shot);
    }
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

// Opens `query` on `pageBackend` and waits until it runs and drew.
async function open(query, pageBackend) {
    const page = new URL(url);
    page.search = query;
    page.searchParams.set("backend", pageBackend);
    await cdp.send("Page.navigate", { url: page.href });
    await cdp.until(`location.href === ${JSON.stringify(page.href)}
        && window.__cranposeAppState?.phase === 'running'`);
    await pause(SETTLE_MS);
}

async function leave() {
    await cdp.send("Page.navigate", { url: "about:blank" });
    await cdp.until("location.href === 'about:blank'");
    await pause(LEAVE_MS);
}

// The page's screenshot as the mean color of each cell of a coarse grid.
async function cells() {
    const { data } = await cdp.send("Page.captureScreenshot", {
        format: "png",
        clip: { x: 0, y: 0, width: WIDTH, height: HEIGHT, scale: 0.25 },
    });
    return cdp.evaluate(`(async () => {
        const image = new Image();
        image.src = 'data:image/png;base64,${data}';
        await image.decode();
        const context = new OffscreenCanvas(16, 20).getContext('2d');
        context.drawImage(image, 0, 0, 16, 20);
        return [...context.getImageData(0, 0, 16, 20).data];
    })()`);
}

// The default page drawn on WebGL against the same page on WebGPU: the mean
// difference of their coarse pictures, and of the WebGL one turned upside
// down. A frame left upside down is far from WebGPU's and close to it turned.
async function sameAsWebGpu() {
    const start = cdp.logs.length;
    const errors = [];
    await open("", "gl");
    const gl = await cells();
    const adapter = await cdp.evaluate("navigator.gpu?.requestAdapter().then(adapter => !!adapter) ?? false");
    await leave();
    if (!adapter) {
        console.log("skip (orientation) Chrome offers no WebGPU adapter to compare with");
        return;
    }
    await open("", "webgpu");
    const webgpu = await cells();
    await leave();
    const rowBytes = 16 * 4;
    const turned = gl.map((_, index) => gl[(19 - Math.floor(index / rowBytes)) * rowBytes + (index % rowBytes)]);
    const difference = picture => picture.reduce((sum, value, index) => sum + Math.abs(value - webgpu[index]), 0) / picture.length;
    const [upright, upsideDown] = [difference(gl), difference(turned)];
    if (!(upright < 8 && upright * 2 < upsideDown)) {
        errors.push(`WebGL's default page differs from WebGPU's by ${upright.toFixed(1)} a channel, and by ${upsideDown.toFixed(1)} turned upside down`);
    }
    results.push({ page: "(orientation)", errors, console: cdp.logs.slice(start) });
    console.log(`${errors.length ? "FAIL" : "ok  "} (orientation) difference ${upright.toFixed(1)}, upside down ${upsideDown.toFixed(1)}`);
    for (const error of errors) console.log("     " + error);
}

// A press at the middle of the counter page's Increment button, where the
// page shows it, counts once.
async function pressCounts() {
    const start = cdp.logs.length;
    const errors = [];
    await open("tab=counter", backend);
    await cdp.until(ENABLE_MIRROR);
    const increment = `document.querySelector('[data-cranpose-node][aria-label="Increment"]')`;
    const count = `Number(document.querySelector('[data-cranpose-accessibility]').textContent.match(/Counter: (-?\\d+)/)[1])`;
    await cdp.until(`!!${increment}`);
    const [x, y, before] = await cdp.evaluate(`(() => {
        const rect = ${increment}.getBoundingClientRect();
        return [rect.left + rect.width / 2, rect.top + rect.height / 2, ${count}];
    })()`);
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
        await cdp.send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
    }
    try {
        await cdp.until(`${count} === ${before + 1}`);
    } catch {
        errors.push(`a press on Increment at ${Math.round(x)}, ${Math.round(y)} left the count at ${await cdp.evaluate(count)}, from ${before}`);
    }
    await leave();
    errors.push(...cdp.logs.slice(start).filter(line => FAILURE.test(line)));
    results.push({ page: "(press)", errors, console: cdp.logs.slice(start) });
    console.log(`${errors.length ? "FAIL" : "ok  "} (press)`);
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
    await pressCounts();
    if (backend === "gl") await sameAsWebGpu();
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
