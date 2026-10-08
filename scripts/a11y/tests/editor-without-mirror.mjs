import assert from "node:assert/strict";
import { ENABLE_MIRROR } from "../cdp.mjs";

const appText = "document.querySelector('[data-cranpose-accessibility]').textContent";
const frames = "new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))";

async function key(send, keyName, code, keyCode, text, modifiers = 0) {
    await send("Input.dispatchKeyEvent", {type: "keyDown", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers, ...(text ? {text} : {})});
    await send("Input.dispatchKeyEvent", {type: "keyUp", key: keyName, code, windowsVirtualKeyCode: keyCode, modifiers});
}

// The centres of the mirror nodes named in `labels`, read from a page the
// mirror is on, so a fresh page without the mirror can be clicked there.
async function centres({send, until, evaluate}, url, labels) {
    await send("Page.navigate", {url});
    await until(ENABLE_MIRROR);
    await until(`${JSON.stringify(labels)}.every(label => document.querySelector('[aria-label="' + label + '"]'))`);
    const points = await evaluate(`${JSON.stringify(labels)}.map(label => {
        const rect = document.querySelector('[aria-label="' + label + '"]').getBoundingClientRect();
        return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2};
    })`);
    await send("Page.navigate", {url});
    await until(`!!document.querySelector('[data-cranpose-enable-accessibility]')`);
    await evaluate(frames);
    return points;
}

async function click(send, point) {
    await send("Input.dispatchMouseEvent", {type: "mousePressed", ...point, button: "left", clickCount: 1});
    await send("Input.dispatchMouseEvent", {type: "mouseReleased", ...point, button: "left", clickCount: 1});
}

export async function checkEditorWithoutMirror({send, until, evaluate, report, url}) {
    const inputUrl = new URL(url);
    inputUrl.searchParams.set("tab", "textinput");
    const [field] = await centres({send, until, evaluate}, inputUrl.href, ["Basic text field"]);
    assert.equal(await evaluate(`document.querySelectorAll('[data-cranpose-accessibility], [data-cranpose-node], [data-cranpose-live]').length`), 0,
        "the page builds no mirror and no live region before a reader asks for them");
    const tree = await send("Accessibility.getFullAXTree");
    assert.ok(tree.nodes.some(node => !node.ignored && node.role?.value === "button" && node.name?.value === "Enable accessibility"),
        "a reader finds the button that turns the mirror on");
    report.push("without a reader the page holds one button that turns the mirror on");

    await click(send, field);
    await until("document.activeElement?.matches('textarea[data-cranpose-editor]')");
    await evaluate("window.editor = document.activeElement; window.editor.select()");
    await send("Input.insertText", {text: "A🌍Z"});
    await until("window.editor.value === 'A🌍Z'");
    await evaluate("window.editor.setSelectionRange(1, 3, 'backward')");
    await evaluate(frames);
    await send("Input.imeSetComposition", {text: "に", selectionStart: 1, selectionEnd: 1});
    await until("window.editor.value === 'AにZ'");
    await send("Input.imeSetComposition", {text: "日本", selectionStart: 2, selectionEnd: 2});
    await until("window.editor.value === 'A日本Z'");
    await send("Input.insertText", {text: "日本語"});
    await until("window.editor.value === 'A日本語Z'");
    await key(send, "Backspace", "Backspace", 8);
    await key(send, "x", "KeyX", 88, "x");
    await until("window.editor.value === 'A日本xZ'");
    await evaluate(frames);
    assert.deepEqual(await evaluate("[document.activeElement === window.editor, window.editor.value, window.editor.selectionStart]"),
        [true, "A日本xZ", 4], "the app's field and its caret come back to the editor unchanged");
    report.push("without the mirror a hidden editor takes typing, keys and IME composition");

    await key(send, "Tab", "Tab", 9);
    await until("document.activeElement === window.editor && window.editor.value === ''");
    await send("Input.insertText", {text: "second"});
    await until("window.editor.value === 'second'");
    report.push("Tab moves the app's focus to the next field, and the editor follows it");

    await evaluate(`document.querySelector('[data-cranpose-enable-accessibility]').click()`);
    await until(`!!document.querySelector('[data-cranpose-accessibility]')`);
    await until(`${appText}.includes('Current value: "A日本xZ"') && ${appText}.includes('Field 2 value: "second"')`);
    await until(`document.activeElement?.getAttribute('aria-label') === 'Empty text field'`);
    assert.equal(await evaluate("!window.editor.isConnected && !document.querySelector('[data-cranpose-editor], [data-cranpose-enable-accessibility]')"), true,
        "the button and the hidden editors leave once the mirror is on");
    const mirrored = await send("Accessibility.getFullAXTree");
    assert.ok(mirrored.nodes.some(node => !node.ignored && node.role?.value === "textbox" && node.name?.value === "Empty text field"
        && node.properties?.some(property => property.name === "focused" && property.value.value)),
        "the reader lands on the focused field's node");
    await send("Input.insertText", {text: "!"});
    await until(`${appText}.includes('Field 2 value: "second!"')`);
    report.push("turning the mirror on with a focused field hands typing to the field's node");

    const robotUrl = new URL(url);
    robotUrl.searchParams.set("tab", "accessibility_robot");
    const [passphrase] = await centres({send, until, evaluate}, robotUrl.href, ["Passphrase"]);
    await click(send, passphrase);
    await until("document.activeElement?.matches('input[data-cranpose-editor][type=\"password\"]')");
    assert.deepEqual(await evaluate("[document.activeElement.value, document.activeElement.autocomplete]"),
        ["robot-secret-value", "current-password"], "a secret field opens in a password editor that holds its text");
    await evaluate("window.secret = document.activeElement; window.secret.select()");
    await send("Input.insertText", {text: "private🌍"});
    await until("window.secret.value === 'private🌍'");
    await key(send, "Tab", "Tab", 9, undefined, 8);
    await until("document.activeElement?.matches('[data-cranpose-editor]') && document.activeElement !== window.secret");
    assert.notEqual(await evaluate("document.activeElement.type"), "password", "a plain field leaves the password editor");
    await evaluate("document.activeElement.select()");
    await send("Input.insertText", {text: "plain"});
    await until("document.activeElement.value === 'plain'");
    await click(send, passphrase);
    await until("document.activeElement === window.secret && window.secret.value === 'private🌍'");
    report.push("a secret field takes typing through a password editor, and a plain field through a plain one");

    await evaluate(`document.querySelector('[data-cranpose-enable-accessibility]').click()`);
    await until(`!!document.querySelector('[data-cranpose-accessibility]')`);
    await until(`${appText}.includes('Edited: plain') && document.querySelector('input[aria-label="Passphrase"]')?.value === 'private🌍'`);
    assert.equal(await evaluate("document.activeElement?.getAttribute('aria-label')"), "Passphrase",
        "the reader lands on the secret field's own password input");
    report.push("what was typed into the secret field reached the app");
}
