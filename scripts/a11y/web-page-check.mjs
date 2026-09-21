#!/usr/bin/env node
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { mkdtempSync, openSync, readFileSync, writeFileSync, closeSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const url = process.argv[2];
assert.ok(url, "pass the URL of a running web demo");
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
let port = Number(process.env.A11Y_DEBUG_PORT ?? 0);
const output = process.argv[3];
const profile = mkdtempSync(join(tmpdir(), "a11y-chrome-"));
const browserLog = output ? openSync(join(output, 'chrome.log'), 'w') : null;
let browserError;

const browser = spawn(
  chrome,
  [
    "--headless=new",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "--no-default-browser-check",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--window-size=1024,700",
    "about:blank",
  ],
  { stdio: ['ignore', 'ignore', browserLog ?? 'inherit'] },
);
browser.on('error', error => { browserError = error; });

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function pageTarget() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    if (browserError) throw browserError;
    if (browser.exitCode !== null || browser.signalCode !== null) throw new Error('Chrome exited before connecting');
    try {
      if (port === 0) port = Number(readFileSync(join(profile, 'DevToolsActivePort'), 'utf8').split('\n')[0]);
      const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json());
      const page = list.find((t) => t.type === "page");
      if (page) return page;
    } catch {}
    await pause(200);
  }
  throw new Error("chrome did not answer on the debugging port");
}

let ws;
async function connect() {
  const target = await pageTarget();
  ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("debugger connection timed out")), 10000);
    ws.onopen = () => { clearTimeout(timeout); resolve(); };
    ws.onerror = (error) => { clearTimeout(timeout); reject(error); };
  });
  ws.onmessage = receive;
}
let nextId = 1;
const pending = new Map();
const consoleLines = [];
function receive(event) {
  const message = JSON.parse(event.data);
  if (message.method === "Runtime.consoleAPICalled") {
    consoleLines.push(`${message.params.type}: ${message.params.args.map((a) => a.value ?? a.description ?? "").join(" ")}`);
  }
  if (message.method === "Runtime.exceptionThrown") {
    const details = message.params.exceptionDetails;
    consoleLines.push(`exception: ${details.text} ${details.exception?.description ?? ""}`);
  }
  if (message.id && pending.has(message.id)) {
    const { resolve, reject } = pending.get(message.id);
    pending.delete(message.id);
    if (message.error) reject(new Error(JSON.stringify(message.error)));
    else resolve(message.result);
  }
};
function send(method, params = {}) {
  const id = nextId++;
  ws.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 15000);
    pending.set(id, {
      resolve: (value) => { clearTimeout(timer); resolve(value); },
      reject: (error) => { clearTimeout(timer); reject(error); },
    });
  });
}
async function evaluate(expression) {
  const result = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
async function key(keyName, code, keyCode, modifiers = 0) {
  await send("Input.dispatchKeyEvent", {
    type: "keyDown", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers,
    ...(keyName === "Enter" ? { text: "\r" } : {}),
  });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers });
}

const report = [];
const counter = `Number(document.querySelector('[data-cranpose-accessibility]').textContent.match(/Counter: (-?\\d+)/)[1])`;
async function check(name, expression) {
  assert.equal(await evaluate(expression), true, name);
  report.push(name);
}
async function until(expression) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (await evaluate(expression)) return;
    await pause(200);
  }
  throw new Error(`condition did not become true: ${expression}`);
}
try {
  await connect();
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.navigate", { url });
  await until(`!!document.querySelector('[data-cranpose-node][aria-label="Increment"]')`);
  await pause(500);
  await evaluate(`(() => {
    window.a11yButton = document.querySelector('[data-cranpose-node][aria-label="Increment"]');
    window.a11yBefore = ${counter};
    window.a11yButton.focus();
    window.a11yButton.click();
  })()`);
  await until(`${counter} === window.a11yBefore + 1`);
  await check("a state change preserves the reader's control", `window.a11yButton === document.querySelector('[data-cranpose-node][aria-label="Increment"]')`);
  await check("a state change preserves focus", `document.activeElement === window.a11yButton`);
  await key("Tab", "Tab", 9);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Decrement'`);
  report.push("Tab advances to the next control");
  await key("Enter", "Enter", 13);
  await until(`${counter} === window.a11yBefore`);
  report.push("Enter activates the focused control once");
  await key("Tab", "Tab", 9, 8);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Increment'`);
  report.push("Shift+Tab returns to the previous control");
  await check("tab indices are valid integers", `[...document.querySelectorAll('[data-cranpose-node][tabindex]')].every(node => /^-?\\d+$/.test(node.getAttribute('tabindex')))`);
  await check("no invalid text role", `!document.querySelector('[data-cranpose-node][role="text"]')`);
  const fixtureUrl = new URL(url);
  fixtureUrl.searchParams.set('tab', 'accessibility_robot');
  await send('Page.navigate', { url: fixtureUrl.href });
  await until(`!!document.querySelector('[data-cranpose-node][aria-label="Remove"]')`);
  const nativeTree = await send('Accessibility.getFullAXTree');
  const accessible = nativeTree.nodes.filter(node => !node.ignored);
  assert.ok(accessible.some(node => node.name?.value === 'Account, Account'), 'merged native name');
  assert.ok(accessible.some(node => node.name?.value === 'Remove' && node.role?.value === 'button'), 'independent nested native control');
  assert.ok(!JSON.stringify(accessible).includes('robot-secret-value'), 'password values stay out of the browser accessibility tree');
  assert.ok(!JSON.stringify(accessible).includes('Decorative secret'), 'hidden content stays out of the browser accessibility tree');
  const loading = accessible.find(node => node.name?.value === 'Loading' && node.role?.value === 'progressbar');
  assert.equal(loading?.value?.value, 40, 'native passive progress range');
  report.push('the browser accessibility tree preserves names, roles, and private content boundaries');
  await evaluate(`document.querySelector('[aria-label="Disabled action"]').click()`);
  await pause(200);
  await check('disabled native controls reject activation', `document.querySelector('[aria-label="Disabled action"]').getAttribute('aria-disabled') === 'true' && document.querySelector('[data-cranpose-accessibility]').textContent.includes('Action count: 0')`);
  await evaluate(`document.querySelector('[aria-label="Volume"]').focus()`);
  await key('ArrowRight', 'ArrowRight', 39);
  await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('Volume value: 40')`);
  report.push('keyboard range adjustment reaches application state');
  const listUrl = new URL(url);
  listUrl.searchParams.set("tab", "lazylist");
  await send("Page.navigate", { url: listUrl.href });
  await until(`!!document.querySelector('[data-cranpose-node][role="list"]')`);
  await check("list children belong to their container", `!!document.querySelector('[data-cranpose-node][role="list"] [data-cranpose-node]')`);
  const inputUrl = new URL(url);
  inputUrl.searchParams.set("tab", "textinput");
  await send("Page.navigate", { url: inputUrl.href });
  await until(`!!document.querySelector('[data-cranpose-node] input, input[data-cranpose-node], textarea[data-cranpose-node]')`);
  await evaluate(`(() => {
    window.a11yField = document.querySelector('input[data-cranpose-node], textarea[data-cranpose-node]');
    window.a11yField.focus();
    window.a11yField.value = 'Voice input café 🌍';
    window.a11yField.setSelectionRange(5, 10, 'backward');
    window.a11yField.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertReplacementText' }));
  })()`);
  await pause(500);
  await check("dictated input reaches the application", `document.querySelector('[data-cranpose-accessibility]').textContent.includes('Voice input café 🌍')`);
  await check("editing preserves the native field", `window.a11yField.isConnected && window.a11yField.value === 'Voice input café 🌍'`);
  await check("editing preserves backward selection", `window.a11yField.selectionStart === 5 && window.a11yField.selectionEnd === 10 && window.a11yField.selectionDirection === 'backward'`);
  await key("x", "KeyX", 88);
  await until(`window.a11yField.value === 'Voicext café 🌍'`);
  await check("keyboard input reaches the application once", `document.querySelector('[data-cranpose-accessibility]').textContent.includes('Voicext café 🌍')`);
  const failures = consoleLines.filter(line => /panicked|exception/.test(line));
  assert.deepEqual(failures, [], "the application must not panic");
  const result = { platform: 'web', status: 'passed', passed: report };
  if (output) writeFileSync(join(output, 'report.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  if (output) writeFileSync(join(output, 'report.json'), JSON.stringify({ platform: 'web', status: 'failed', passed: report, error: String(error) }, null, 2));
  throw error;
} finally {
  if (output && ws?.readyState === WebSocket.OPEN) {
    try {
      const tree = await send('Accessibility.getFullAXTree');
      writeFileSync(join(output, 'native-tree.json'), JSON.stringify(tree, null, 2));
      const screenshot = await send('Page.captureScreenshot');
      writeFileSync(join(output, 'screen.png'), Buffer.from(screenshot.data, 'base64'));
    } catch (error) { console.error('Artifact capture:', error); }
  }
  ws?.close();
  if (!browserError && browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(resolve => browser.once("exit", resolve));
    browser.kill();
    await exited;
  }
  if (browserLog !== null) closeSync(browserLog);
}
