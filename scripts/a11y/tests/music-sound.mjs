#!/usr/bin/env node
// Opens the Music tab of the packaged web demo and presses Play the way a
// user does. A browser gives a page no sound before such a press, so the
// press must open the output: the check fails unless the page then schedules
// blocks of sound that are not silent, one after another without a hole, and
// the tab's clock moves with them. Pause must let the output rest again.
// Chrome runs with its sound output off: it takes the samples at the pace of
// a device and plays nothing, on a host with a sound device and on one without.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { ENABLE_MIRROR } from "../cdp.mjs";
import { GPU_FLAGS, launchChrome } from "../chrome.mjs";

const url = process.argv[2];
const output = process.argv[3];
assert.ok(url && output, "pass the URL of the packaged web demo and an output directory");

const WIDTH = 1280;
const HEIGHT = 800;
const LISTEN_MS = 3000;
// A block starts where the one before it ends; the clock of the page counts
// in doubles, so the two differ by rounding alone.
const SEAM_SECONDS = 1e-4;
// A loaded runner may stall the page past the lead of the output and leave a
// hole. Each hole costs one seam; more than this share of them is a defect.
const MOST_HOLES = 0.05;

// Counts what the page hands to Web Audio, before the page's own code runs.
const PROBE = `
window.__musicProbe = { blocks: 0, peak: 0, starts: [], contexts: [] };
{
    const probe = window.__musicProbe;
    const copy = AudioBuffer.prototype.copyToChannel;
    AudioBuffer.prototype.copyToChannel = function (source, channel, ...rest) {
        if (channel === 0) {
            probe.blocks += 1;
            for (const sample of source) probe.peak = Math.max(probe.peak, Math.abs(sample));
        }
        return copy.call(this, source, channel, ...rest);
    };
    const start = AudioBufferSourceNode.prototype.start;
    AudioBufferSourceNode.prototype.start = function (when, ...rest) {
        probe.starts.push({ when, seconds: this.buffer.duration, now: this.context.currentTime });
        return start.call(this, when, ...rest);
    };
    const Context = window.AudioContext;
    window.AudioContext = class extends Context {
        constructor(...args) {
            super(...args);
            probe.contexts.push(this);
        }
    };
}`;

// The middle of the mirror node that reads `label`, or null.
const middleOf = label => `(() => {
    const label = ${JSON.stringify(label)};
    const node = [...document.querySelectorAll('[data-cranpose-node]')].find(node =>
        node.getAttribute('aria-label') === label || node.textContent.trim() === label);
    if (!node) return null;
    const box = node.getBoundingClientRect();
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
})()`;

// The seconds the tab's clock shows, or null before it shows any.
const CLOCK_SECONDS = `(() => {
    for (const node of document.querySelectorAll('[data-cranpose-node]')) {
        const clock = /^(\\d+):(\\d\\d) \\/ \\d+:\\d\\d$/.exec(
            node.getAttribute('aria-label') ?? node.textContent.trim());
        if (clock) return Number(clock[1]) * 60 + Number(clock[2]);
    }
    return null;
})()`;

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const { cdp, close } = await launchChrome(
    [`--window-size=${WIDTH},${HEIGHT}`, "--disable-audio-output", ...GPU_FLAGS], output);

// A press of the real pointer: only that counts as the user's act.
async function press(label) {
    await cdp.until(`!!${middleOf(label)}`);
    const { x, y } = await cdp.evaluate(middleOf(label));
    const button = { x, y, button: "left", clickCount: 1 };
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", ...button });
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", ...button });
}

let failure;
try {
    const page = new URL(url);
    page.searchParams.set("tab", "music");
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: PROBE });
    await cdp.send("Page.navigate", { url: page.href });
    await cdp.until("window.__cranposeAppState?.phase === 'running'");
    await cdp.until(ENABLE_MIRROR);
    await cdp.until(`${CLOCK_SECONDS} === 0`);
    assert.equal(await cdp.evaluate("window.__musicProbe.contexts.length"), 0,
        "the tab opens no output before a sound");

    await press("Play");
    await cdp.until("window.__musicProbe.contexts[0]?.state === 'running' && window.__musicProbe.blocks > 0");
    await pause(LISTEN_MS);
    const shot = await cdp.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(output, "music-playing.png"), Buffer.from(shot.data, "base64"));

    const heard = await cdp.evaluate(`(() => {
        const { blocks, peak, starts, contexts } = window.__musicProbe;
        let holes = 0;
        let late = [];
        for (let index = 0; index < starts.length; index += 1) {
            if (starts[index].when < starts[index].now) late.push({ index, ...starts[index] });
            if (index === 0) continue;
            const seam = starts[index - 1].when + starts[index - 1].seconds;
            if (Math.abs(starts[index].when - seam) > ${SEAM_SECONDS}) holes += 1;
        }
        return { blocks, peak, holes, late, sampleRate: contexts[0].sampleRate, clock: ${CLOCK_SECONDS} };
    })()`);
    console.log("heard:", JSON.stringify(heard));
    const blockSeconds = 1024 / heard.sampleRate;
    assert.ok(heard.blocks >= 0.8 * LISTEN_MS / 1000 / blockSeconds,
        `the page schedules a block for each ${blockSeconds.toFixed(4)} s of sound: ${heard.blocks}`);
    assert.ok(heard.peak > 0.01, `the blocks carry the piece, not silence: peak ${heard.peak}`);
    assert.ok(heard.peak <= 1, `the blocks stay within full scale: peak ${heard.peak}`);
    assert.deepEqual(heard.late, [], "no block is scheduled for a time already past");
    assert.ok(heard.holes <= Math.max(1, heard.blocks * MOST_HOLES),
        `the blocks follow one another: ${heard.holes} holes in ${heard.blocks} blocks`);
    assert.ok(heard.clock >= 2 && heard.clock <= 4,
        `the tab's clock follows the sound: ${heard.clock} s after ${LISTEN_MS} ms`);

    await press("Pause");
    await cdp.until("window.__musicProbe.contexts[0].state === 'suspended'");
    console.log("ok   the Music tab sounds after a press and rests after a pause");
} catch (error) {
    failure = error;
    console.log("FAIL " + error.message);
    for (const line of cdp.logs.slice(-40)) console.log("     " + line);
} finally {
    await close();
}
if (failure) process.exit(1);
