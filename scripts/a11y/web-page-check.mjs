#!/usr/bin/env node
import { spawn } from "node:child_process";
import assert from "node:assert/strict";
import { mkdtempSync, openSync, readFileSync, writeFileSync, closeSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checkMarkdownImages } from "./tests/markdown-images.mjs";
import { checkImeFocus } from "./tests/ime-focus.mjs";
import { Cdp } from "./cdp.mjs";

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

const cdp = new Cdp();
const send = cdp.send.bind(cdp);
const evaluate = cdp.evaluate.bind(cdp);
const until = cdp.until.bind(cdp);
const consoleLines = cdp.logs;
async function key(keyName, code, keyCode, modifiers = 0) {
  await send("Input.dispatchKeyEvent", {
    type: "keyDown", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers,
    ...(keyName === "Enter" ? { text: "\r" }
      : keyName.length === 1 && (modifiers & 7) === 0 ? { text: keyName } : {}),
  });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers });
}

const report = [];
const counter = `Number(document.querySelector('[data-cranpose-accessibility]').textContent.match(/Counter: (-?\\d+)/)[1])`;
async function check(name, expression) {
  assert.equal(await evaluate(expression), true, name);
  report.push(name);
}
try {
  await cdp.connect((await pageTarget()).webSocketDebuggerUrl);
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Log.enable");
  await checkImeFocus({ send, until, evaluate, report, url });
  await checkMarkdownImages({ send, until, evaluate, report, url, output });
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
  await evaluate(`document.querySelector('[aria-label="Express"]').focus()`);
  await key('ArrowRight', 'ArrowRight', 39);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Collection' && document.activeElement.getAttribute('aria-checked') === 'true'`);
  await key('ArrowRight', 'ArrowRight', 39);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Standard' && document.activeElement.getAttribute('aria-checked') === 'true'`);
  await key('End', 'End', 35);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Collection'`);
  await key('Home', 'Home', 36);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Standard'`);
  await check('radio groups retain one Tab stop and a native owner', `document.querySelectorAll('[role="radiogroup"] [role="radio"][tabindex="0"]').length === 1`);
  await key('Tab', 'Tab', 9);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Open preferences'`);
  report.push('radio keyboard navigation wraps, selects, and exits the group in one step');
  await key('Enter', 'Enter', 13);
  await until(`!!document.querySelector('[aria-label="Close preferences"]') && !document.querySelector('[aria-label="Increase"]')`);
  await check('modal controls belong to the modal', `!!document.querySelector('[role="dialog"][aria-modal="true"] [aria-label="Close preferences"]')`);
  await evaluate(`document.querySelector('[aria-label="Dialog notes"]').focus()`);
  await key('Tab', 'Tab', 9);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Close preferences'`);
  await evaluate(`document.querySelector('[aria-label="Close preferences"]').focus()`);
  await key('Tab', 'Tab', 9);
  await check('Tab remains inside the modal', `!!document.activeElement.closest('[role="dialog"][aria-modal="true"]')`);
  await evaluate(`document.querySelector('[aria-label="Open confirmation"]').focus(); document.activeElement.click()`);
  await until(`!!document.querySelector('[aria-label="Close confirmation"]') && !document.querySelector('[aria-label="Close preferences"]')`);
  await key('Escape', 'Escape', 27);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Open confirmation'`);
  await key('Escape', 'Escape', 27);
  await until(`document.activeElement?.getAttribute('aria-label') === 'Open preferences' && !!document.querySelector('[aria-label="Increase"]')`);
  report.push('nested modals isolate the reader tree and restore opener focus');
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
  await until(`window.a11yField.value === 'Voicext café 🌍' && document.querySelector('[data-cranpose-accessibility]').textContent.includes('Voicext café 🌍')`);
  await check("keyboard input reaches the application once", `document.querySelector('[data-cranpose-accessibility]').textContent.includes('Voicext café 🌍')`);
  const failures = consoleLines.filter(line => /panicked|exception|Blocked aria-hidden/.test(line));
  assert.deepEqual(failures, [], "the application must not panic or hide focused controls");
  const result = { platform: 'web', status: 'passed', passed: report };
  if (output) writeFileSync(join(output, 'report.json'), JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
} catch (error) {
  if (output) writeFileSync(join(output, 'report.json'), JSON.stringify({ platform: 'web', status: 'failed', passed: report, error: String(error) }, null, 2));
  throw error;
} finally {
  if (output) writeFileSync(join(output, 'console.json'), JSON.stringify(consoleLines, null, 2));
  if (output && cdp.ws?.readyState === WebSocket.OPEN) {
    try {
      const tree = await send('Accessibility.getFullAXTree');
      writeFileSync(join(output, 'native-tree.json'), JSON.stringify(tree, null, 2));
      const screenshot = await send('Page.captureScreenshot');
      writeFileSync(join(output, 'screen.png'), Buffer.from(screenshot.data, 'base64'));
    } catch (error) { console.error('Artifact capture:', error); }
  }
  cdp.close();
  if (!browserError && browser.exitCode === null && browser.signalCode === null) {
    const exited = new Promise(resolve => browser.once("exit", resolve));
    browser.kill();
    await exited;
  }
  if (browserLog !== null) closeSync(browserLog);
}
