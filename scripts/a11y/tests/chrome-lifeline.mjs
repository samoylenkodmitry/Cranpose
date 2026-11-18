#!/usr/bin/env node
// Kills a script that started headless Chrome through `launchChrome` with
// SIGKILL, as a CI cancel or an outer timeout does. Checks that its Chrome
// exits as well. A Chrome that outlives its script keeps rendering until
// someone kills it. On a measuring host its compositing moves the display
// under every later measurement.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import { launchChrome } from "../chrome.mjs";

const EXIT_SECONDS = 10;

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

function alive(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error.code === "EPERM";
    }
}

if (process.argv[2] === "child") {
    const { pid } = await launchChrome([]);
    console.log(pid);
    setInterval(() => {}, 1000);
} else {
    const child = spawn(process.execPath, [fileURLToPath(import.meta.url), "child"],
        { stdio: ["ignore", "pipe", "inherit"] });
    let printed = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", text => { printed += text; });
    while (!printed.includes("\n")) {
        if (child.exitCode !== null) throw new Error(`the child exited with ${child.exitCode} before Chrome started`);
        await pause(100);
    }
    const chrome = Number(printed.trim());
    assert.ok(alive(chrome), "Chrome runs once launchChrome returns");
    const exited = once(child, "exit");
    child.kill("SIGKILL");
    await exited;
    const deadline = Date.now() + EXIT_SECONDS * 1000;
    while (alive(chrome) && Date.now() < deadline) await pause(100);
    if (alive(chrome)) {
        process.kill(chrome, "SIGKILL");
        assert.fail(`Chrome ${chrome} still ran ${EXIT_SECONDS} s after its script got SIGKILL`);
    }
    console.log("ok   Chrome exits when the script that started it is killed");
}
