#!/usr/bin/env node
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const url = process.argv[2] ?? "http://192.168.50.114:8080/";
const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const port = 9333;
const profile = mkdtempSync(join(tmpdir(), "a11y-chrome-"));

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
  { stdio: "ignore" },
);

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function pageTarget() {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    try {
      const list = await fetch(`http://127.0.0.1:${port}/json/list`).then((r) => r.json());
      const page = list.find((t) => t.type === "page");
      if (page) return page;
    } catch {}
    await pause(200);
  }
  throw new Error("chrome did not answer on the debugging port");
}

const target = await pageTarget();
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});
let nextId = 1;
const pending = new Map();
const consoleLines = [];
ws.onmessage = (event) => {
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
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
async function evaluate(expression) {
  const result = await send("Runtime.evaluate", { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
async function key(keyName, code, keyCode) {
  await send("Input.dispatchKeyEvent", { type: "keyDown", key: keyName, code, windowsVirtualKeyCode: keyCode });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key: keyName, code, windowsVirtualKeyCode: keyCode });
}

const report = {};
try {
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Page.navigate", { url });
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await pause(1000);
    const ready = await evaluate(`document.querySelectorAll('[data-cranpose-node]').length > 0`);
    if (ready) break;
  }
  await pause(1500);
  report.mirror = await evaluate(`(() => {
    const nodes = [...document.querySelectorAll('[data-cranpose-node]')];
    return { count: nodes.length, paged: nodes.filter(n => n.hasAttribute('data-cranpose-page')).length,
      counter: nodes.filter(n => n.getAttribute('aria-label') === 'Counter App').map(n => [n.style.left, n.getAttribute('data-cranpose-page'), n.getAttribute('data-cranpose-page-dx'), n.getAttribute('data-cranpose-page-dy')]),
      hacker: nodes.filter(n => n.getAttribute('aria-label') === 'Hacker News').map(n => n.style.left) };
  })()`);
  report.textboxes = await evaluate(`[...document.querySelectorAll('[data-cranpose-node][role="textbox"]')].map(n => [n.getAttribute('aria-label'), n.textContent, n.getAttribute('tabindex')])`);
  report.focused = await evaluate(`(() => { const n = document.querySelector('[data-cranpose-node][aria-label="Counter App"]'); if (!n) return null; n.focus(); return document.activeElement === n; })()`);
  await key("PageDown", "PageDown", 34);
  await pause(1200);
  report.afterPageDown = await evaluate(`(() => {
    const live = [...document.querySelectorAll('[aria-live]')].map(n => n.textContent);
    const counter = document.querySelector('[data-cranpose-node][aria-label="Counter App"]');
    const hacker = document.querySelector('[data-cranpose-node][aria-label="Hacker News"]');
    return { live, counterLeft: counter && counter.style.left, hackerLeft: hacker && hacker.style.left, active: document.activeElement && document.activeElement.getAttribute('aria-label') };
  })()`);
  await key("PageUp", "PageUp", 33);
  await pause(1200);
  report.afterPageUp = await evaluate(`(() => {
    const live = [...document.querySelectorAll('[data-cranpose-live]')].map(n => n.textContent);
    const counter = document.querySelector('[data-cranpose-node][aria-label="Counter App"]');
    return { live, counterLeft: counter && counter.style.left, active: document.activeElement && document.activeElement.getAttribute('aria-label') };
  })()`);
  await evaluate(`(() => { const n = document.querySelector('[data-cranpose-node][aria-label="Liquid UI"]'); if (n) n.click(); return !!n; })()`);
  await pause(1500);
  report.afterClick = await evaluate(`(() => {
    const labels = [...document.querySelectorAll('[data-cranpose-node]')].map(n => n.getAttribute('aria-label')).filter(Boolean);
    const textboxes = [...document.querySelectorAll('[data-cranpose-node][role="textbox"]')].map(n => [n.getAttribute('aria-label'), n.textContent]);
    return { count: labels.length, first: labels.slice(0, 12), textboxes };
  })()`);
  await evaluate(`(() => { const n = document.querySelector('[data-cranpose-node][aria-label="Counter App"]'); if (n) n.click(); return !!n; })()`);
  await pause(1500);
  await evaluate(`(() => { const n = document.querySelector('[data-cranpose-node][aria-label="Text Input"]'); if (n) n.click(); return !!n; })()`);
  await pause(1500);
  report.textInput = await evaluate(`(() => {
    const textboxes = [...document.querySelectorAll('[data-cranpose-node][role="textbox"]')].map(n => [n.getAttribute('aria-label'), n.textContent, n.getAttribute('tabindex')]);
    const labels = [...document.querySelectorAll('[data-cranpose-node]')].map(n => n.getAttribute('aria-label')).filter(Boolean);
    const tab = document.querySelector('[data-cranpose-node][aria-label="Text Input"]');
    const roles = [...new Set([...document.querySelectorAll('[data-cranpose-node]')].map(n => n.getAttribute('role')))];
    return { textboxes, count: labels.length, roles, later: labels.slice(16, 40), tab: tab && [tab.getAttribute('data-cranpose-x'), tab.getAttribute('data-cranpose-y'), tab.style.left], active: document.activeElement && document.activeElement.getAttribute('aria-label') };
  })()`);
} catch (error) {
  report.error = String(error);
} finally {
  const kept = consoleLines.filter((line) => !line.includes("Graphics startup")).map((line) => line.split("\n").slice(0, 3).join(" / "));
  const bad = kept.filter((line) => line.includes("panicked") || line.includes("exception") || line.includes("error"));
  report.console = { total: kept.length, firstBad: bad.slice(0, 3) };
  ws.close();
  browser.kill();
}
console.log(JSON.stringify(report, null, 2));
