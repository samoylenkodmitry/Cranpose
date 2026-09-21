#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Cdp } from "./cdp.mjs";
import { checkImeFocus } from "./tests/ime-focus.mjs";

const [serial, endpoint, url, output, mode] = process.argv.slice(2);
assert.ok(serial && endpoint && url && output && mode, "pass device serial, CDP endpoint, test URL, output, and keyboard layout JSON or inspect");
mkdirSync(output, {recursive: true});
const adb = (...args) => execFileSync("adb", ["-s", serial, ...args], {encoding: "utf8", timeout: 20000});
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const decode = value => value.replaceAll("&quot;", '"').replaceAll("&amp;", "&").replaceAll("&lt;", "<").replaceAll("&gt;", ">");
function nodes() {
    adb("shell", "uiautomator", "dump", "/sdcard/cranpose-ime-window.xml");
    const xml = adb("shell", "cat", "/sdcard/cranpose-ime-window.xml");
    writeFileSync(join(output, "window.xml"), xml);
    return [...xml.matchAll(/<node\s+([^>]+)>/g)].map(match =>
        Object.fromEntries([...match[1].matchAll(/([\w-]+)="([^"]*)"/g)].map(([, key, value]) => [key, decode(value)])));
}
function tap(node) {
    assert.ok(node, "UI node must exist");
    const bounds = node.bounds.match(/\d+/g).map(Number);
    adb("shell", "input", "tap", String(Math.round((bounds[0] + bounds[2]) / 2)), String(Math.round((bounds[1] + bounds[3]) / 2)));
}
function keyboardKey(label) {
    const point = JSON.parse(readFileSync(mode, "utf8"))[label];
    assert.ok(point, "calibrated keyboard key: " + label);
    adb("shell", "input", "tap", String(point[0]), String(point[1]));
}
const cdp = new Cdp();
async function checkAppText(expected) {
    await cdp.until("window.imeField.value.toLowerCase() === " + JSON.stringify(expected));
    const actual = await cdp.evaluate("window.imeField.value");
    assert.equal(await cdp.evaluate("document.querySelector('[data-cranpose-accessibility]').textContent.includes(" +
        JSON.stringify('Current value: "' + actual + '"') + ")"), true);
}
const testIme = process.env.A11Y_ANDROID_IME ?? "com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME";
const originalIme = adb("shell", "settings", "get", "secure", "default_input_method").trim();
const report = {status: "running", serial, originalIme, testIme, passed: [],
    coverage: "Physical software keyboard taps and glide; browser-native IME composition via CDP"};
try {
    const targets = await fetch(endpoint + "/json/list").then(response => response.json());
    const page = targets.find(target => target.type === "page" && target.url.startsWith(url));
    assert.ok(page, "open the test page on the device first");
    await cdp.connect(page.webSocketDebuggerUrl);
    await cdp.send("Page.enable");
    await cdp.send("Runtime.enable");
    await cdp.send("Log.enable");
    await cdp.send("Page.navigate", {url: url + "?tab=textinput"});
    await cdp.until("!!document.querySelector('textarea[data-cranpose-node]')");
    await cdp.evaluate(`(() => {
        window.imeField = document.querySelector('textarea[data-cranpose-node]');
        window.imeEvents = [];
        for (const type of ['focusin', 'focusout', 'compositionstart', 'compositionupdate', 'compositionend', 'beforeinput', 'input']) {
            document.addEventListener(type, event => window.imeEvents.push({
                type, trusted: event.isTrusted, field: event.target === window.imeField,
                hidden: !!event.target.closest?.('[aria-hidden="true"], [hidden], [inert]'),
                data: event.data, inputType: event.inputType, value: event.target.value
            }));
        }
    })()`);
    adb("shell", "ime", "set", testIme);
    assert.equal(adb("shell", "settings", "get", "secure", "default_input_method").trim(), testIme);
    await pause(500);
    tap(nodes().find(node => node.class === "android.widget.EditText" && node.text === "Type here..."));
    await cdp.until("document.activeElement === window.imeField");
    await pause(1000);
    if (mode === "inspect") {
        console.log(JSON.stringify(nodes().filter(node => node.package === "com.google.android.inputmethod.latin"), null, 2));
    } else {
        await cdp.evaluate("window.imeField.select()");
        for (const key of ["c", "a", "t", "Space"]) keyboardKey(key);
        await checkAppText("cat ");
        report.passed.push("software keyboard text reaches application state exactly once");
        const swipe = JSON.parse(readFileSync(mode, "utf8")).swipe;
        assert.ok(swipe, "calibrated keyboard gesture");
        adb("shell", "input", "swipe", ...swipe.map(String));
        await pause(1000);
        await checkAppText("cat to");
        report.passed.push("software keyboard glide reaches application state exactly once");
        assert.equal(await cdp.evaluate(`window.imeEvents.filter(event => ['focusin', 'input', 'compositionstart', 'compositionupdate', 'compositionend'].includes(event.type)).every(event => event.trusted && event.field && !event.hidden)`), true);
        report.passed.push("focus and input never target an aria-hidden helper");
        assert.equal(await cdp.evaluate("document.activeElement === window.imeField && window.imeField.isConnected"), true);
        report.passed.push("native editor retains focus through software keyboard editing");
        report.software_keyboard_events = await cdp.evaluate("window.imeEvents");
        await checkImeFocus({send: cdp.send.bind(cdp), until: cdp.until.bind(cdp),
            evaluate: cdp.evaluate.bind(cdp), report: report.passed, url});
        await cdp.evaluate("document.activeElement.blur()");
        await pause(500);
        const platformNodes = nodes();
        report.password_nodes = platformNodes.filter(node => node.package === "com.android.chrome" && node.password === "true");
        assert.equal(report.password_nodes.length, 1, "Android identifies the native protected password control");
        assert.ok(!platformNodes.some(node => node.text?.includes("private🌍") || node["content-desc"]?.includes("private🌍")),
            "Android accessibility masks the password with this device's password speech setting");
        report.passed.push("Android accessibility exposes the password as protected and masks its text");
    }
    report.status = mode === "inspect" ? "inspection" : "passed";
} catch (error) {
    report.status = "failed";
    report.error = String(error.stack ?? error);
    process.exitCode = 1;
} finally {
    try {
        report.events = await cdp.evaluate("window.imeEvents");
        report.active = await cdp.evaluate("document.activeElement?.outerHTML");
        report.controls = await cdp.evaluate("[...document.querySelectorAll('select, [role=combobox], button')].map(node => node.outerHTML)");
        report.console = cdp.logs;
        assert.ok(!JSON.stringify(cdp.logs).includes("Blocked aria-hidden"), "no blocked aria-hidden warning");
        writeFileSync(join(output, "screen.png"), execFileSync("adb", ["-s", serial, "exec-out", "screencap", "-p"], {timeout: 20000}));
    } catch (error) {
        report.artifactError = String(error);
        if (report.status === "passed") { report.status = "failed"; process.exitCode = 1; }
    }
    adb("shell", "ime", "set", originalIme);
    cdp.close();
    writeFileSync(join(output, "report.json"), JSON.stringify(report, null, 2) + "\n");
    console.log(JSON.stringify({status: report.status, passed: report.passed, error: report.error}, null, 2));
}
