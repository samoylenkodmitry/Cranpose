import { spawn } from "node:child_process";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Cdp } from "./cdp.mjs";

const pause = ms => new Promise(resolve => setTimeout(resolve, ms));

// Starts headless Chrome on a fresh profile with `flags` and connects to its page.
// Chrome's own stderr goes to `output`/chrome.log when an output directory is given.
export async function launchChrome(flags, output) {
    const chrome = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    let port = Number(process.env.A11Y_DEBUG_PORT ?? 0);
    const profile = mkdtempSync(join(tmpdir(), "a11y-chrome-"));
    const browserLog = output ? openSync(join(output, "chrome.log"), "w") : null;
    let browserError;
    const browser = spawn(
        chrome,
        [
            "--headless=new",
            `--remote-debugging-port=${port}`,
            `--user-data-dir=${profile}`,
            "--no-first-run",
            "--no-default-browser-check",
            // Without these Chrome on a Linux runner waits for a desktop keyring
            // and never loads a page.
            "--password-store=basic",
            "--use-mock-keychain",
            ...flags,
            "about:blank",
        ],
        { stdio: ["ignore", "ignore", browserLog ?? "inherit"] },
    );
    browser.on("error", error => { browserError = error; });
    const cdp = new Cdp();

    async function close() {
        cdp.close();
        if (!browserError && browser.exitCode === null && browser.signalCode === null) {
            const exited = new Promise(resolve => browser.once("exit", resolve));
            browser.kill();
            await exited;
        }
        if (browserLog !== null) closeSync(browserLog);
        // Chrome's helper processes can still write into the profile after
        // the browser exits; rmSync retries the ENOTEMPTY that leaves.
        rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    }

    try {
        let page;
        for (let attempt = 0; attempt < 50 && !page; attempt += 1) {
            if (browserError) throw browserError;
            if (browser.exitCode !== null || browser.signalCode !== null) throw new Error("Chrome exited before connecting");
            try {
                if (port === 0) port = Number(readFileSync(join(profile, "DevToolsActivePort"), "utf8").split("\n")[0]);
                const list = await fetch(`http://127.0.0.1:${port}/json/list`).then(response => response.json());
                page = list.find(target => target.type === "page");
            } catch {}
            if (!page) await pause(200);
        }
        if (!page) throw new Error("chrome did not answer on the debugging port");
        await cdp.connect(page.webSocketDebuggerUrl);
        await cdp.send("Page.enable");
        await cdp.send("Runtime.enable");
        await cdp.send("Log.enable");
    } catch (error) {
        await close();
        throw error;
    }
    return { cdp, close };
}
