#!/usr/bin/env node
// Opens the packaged web demo in a browser that cannot run it. Safari's
// Lockdown Mode removes WebAssembly from a page, and a reader can turn
// scripts off. Either way the page must say why the demo cannot start under
// one heading, and never spin on as if it were still loading.
// Chrome stands in for Safari: a script that runs before the page's own
// removes WebAssembly, as Lockdown Mode does.
import assert from "node:assert/strict";
import { launchChrome } from "../chrome.mjs";

const url = process.argv[2];
const output = process.argv[3];
assert.ok(url && output, "pass the URL of the packaged web demo and an output directory");

// The lines of text the page shows a reader.
const SHOWN_LINES = `document.body.innerText.split("\\n").map(line => line.trim()).filter(Boolean)`;

const { cdp, close } = await launchChrome([], output);

let failure;
try {
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: "delete globalThis.WebAssembly;" });
    await cdp.send("Page.navigate", { url });
    await cdp.until("window.__cranposeAppState?.phase === 'error'");
    const [heading, reason, ...rest] = await cdp.evaluate(SHOWN_LINES);
    assert.equal(heading, "Cranpose couldn’t start", "the page shows one heading");
    assert.match(reason, /WebAssembly.*Lockdown Mode/, "the page names the cause and Safari's Lockdown Mode");
    assert.deepEqual(rest, ["Reload"], "the page offers a reload and nothing else");
    console.log("ok   a page without WebAssembly names Lockdown Mode");

    await cdp.send("Emulation.setScriptExecutionDisabled", { value: true });
    await cdp.send("Page.navigate", { url });
    await cdp.until("document.readyState === 'complete'");
    assert.deepEqual(await cdp.evaluate(SHOWN_LINES), [
        "Cranpose",
        "The demo needs JavaScript. Turn JavaScript on in the browser’s settings, then reload the page.",
    ], "a page without scripts asks for them and shows no spinner text");
    console.log("ok   a page without scripts asks for JavaScript");
} catch (error) {
    failure = error;
    console.log("FAIL " + error.message);
    for (const line of cdp.logs.slice(-40)) console.log("     " + line);
} finally {
    await close();
}
if (failure) process.exit(1);
