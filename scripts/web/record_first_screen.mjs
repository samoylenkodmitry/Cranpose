// node record_first_screen.mjs SITE PAGE OUT
//
// Records the pipelines a Cranpose web app's first screen draws with, for
// its next build to embed (CRANPOSE_FIRST_SCREEN_PIPELINES): serves the
// built site in SITE, opens SITE/PAGE in headless Chrome on WebGL at a
// phone's and a desktop's viewport, and writes each run's first screen to
// OUT, one set after another. On WebGL the page builds them as it starts, so
// the browser links them while the page composes its first screen. Chrome's
// own output of the last run goes to chrome.log beside OUT.
import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { dirname, extname, join, normalize } from "node:path";
import { launchChrome } from "../a11y/chrome.mjs";

const [site, page, out] = process.argv.slice(2);
if (!site || !page || !out) {
    console.error("usage: node record_first_screen.mjs SITE PAGE OUT");
    process.exit(2);
}

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json" };

// The viewports a first screen is recorded at: a phone's, with its user
// agent, and a desktop window's.
const VIEWPORTS = [
    {
        width: 412, height: 915, deviceScaleFactor: 2.625, mobile: true,
        userAgent: "Mozilla/5.0 (Linux; Android 14; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
    },
    { width: 1280, height: 800, deviceScaleFactor: 2, mobile: false },
];

const server = createServer(async (request, response) => {
    const path = normalize(decodeURIComponent(new URL(request.url, "http://localhost").pathname));
    if (path.split("/").includes("..")) {
        response.writeHead(403);
        response.end();
        return;
    }
    try {
        const body = await readFile(join(site, path));
        response.writeHead(200, { "Content-Type": TYPES[extname(path)] ?? "application/octet-stream" });
        response.end(body);
    } catch {
        response.writeHead(404);
        response.end();
    }
});
await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const sets = [];
try {
    const url = new URL(page, `http://127.0.0.1:${server.address().port}/`);
    url.searchParams.set("backend", "gl");
    for (const viewport of VIEWPORTS) {
        const { cdp, close } = await launchChrome([`--window-size=${viewport.width},${viewport.height}`], dirname(out));
        try {
            const { userAgent, ...metrics } = viewport;
            await cdp.send("Emulation.setDeviceMetricsOverride", metrics);
            if (userAgent) await cdp.send("Emulation.setUserAgentOverride", { userAgent });
            await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: "globalThis.__cranposeRecordFirstScreen = true;" });
            await cdp.send("Page.navigate", { url: url.href });
            await cdp.until("globalThis.__cranposeFirstScreenPipelines !== undefined");
            const bytes = await cdp.evaluate("Array.from(globalThis.__cranposeFirstScreenPipelines)");
            sets.push(Buffer.from(bytes));
            console.log(`${viewport.width}x${viewport.height}: ${bytes.length} bytes`);
        } finally {
            await close();
        }
    }
} finally {
    server.close();
}
await writeFile(out, Buffer.concat(sets));
