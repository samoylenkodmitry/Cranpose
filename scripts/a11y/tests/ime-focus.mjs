import assert from "node:assert/strict";

export async function checkImeFocus({send, until, evaluate, report, url}) {
    const inputUrl = new URL(url);
    inputUrl.searchParams.set("tab", "textinput");
    await send("Page.navigate", {url: inputUrl.href});
    await until(`!!document.querySelector('input[data-cranpose-node], textarea[data-cranpose-node]')`);
    const point = await evaluate(`(() => {
        window.imeField = document.querySelector('input[data-cranpose-node], textarea[data-cranpose-node]');
        window.imeEvents = [];
        for (const type of ['focusin', 'compositionstart', 'compositionupdate', 'compositionend', 'input']) {
            document.addEventListener(type, event => window.imeEvents.push({
                type, trusted: event.isTrusted, field: event.target === window.imeField,
                hidden: !!event.target.closest?.('[aria-hidden="true"], [hidden], [inert]'),
                value: event.target.value, start: event.target.selectionStart, end: event.target.selectionEnd,
            }));
        }
        const rect = window.imeField.getBoundingClientRect();
        return {x: rect.left + rect.width / 2, y: rect.top + rect.height / 2};
    })()`);
    await send("Input.dispatchMouseEvent", {type: "mousePressed", ...point, button: "left", clickCount: 1});
    await send("Input.dispatchMouseEvent", {type: "mouseReleased", ...point, button: "left", clickCount: 1});
    await until("document.activeElement?.matches('input, textarea')");
    assert.equal(await evaluate("document.activeElement === window.imeField"), true,
        "pointer focus must reach the accessible editor: " + JSON.stringify(await evaluate("window.imeEvents")));
    assert.equal(await evaluate(`document.querySelectorAll('textarea:not([data-cranpose-node])').length`), 0,
        "keyboard, IME, and accessibility must share the actual field, without a helper textarea");
    assert.equal(await evaluate("window.imeEvents.filter(e => e.type === 'focusin').every(e => !e.hidden)"), true);
    const tree = await send("Accessibility.getFullAXTree");
    const focused = tree.nodes.filter(node => !node.ignored && node.role?.value === "textbox"
        && node.properties?.some(property => property.name === "focused" && property.value.value));
    assert.equal(focused.length, 1);
    assert.equal(focused[0].name?.value, await evaluate("window.imeField.getAttribute('aria-label')"));

    await evaluate("window.imeField.select()");
    await send("Input.insertText", {text: "A🌍Z"});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('A🌍Z')`);
    await evaluate("window.imeField.setSelectionRange(1, 3, 'backward')");
    await evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
    await send("Input.imeSetComposition", {text: "に", selectionStart: 1, selectionEnd: 1});
    await until("window.imeField.value === 'AにZ'");
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('AにZ')`);
    await send("Input.imeSetComposition", {text: "日本", selectionStart: 2, selectionEnd: 2});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('A日本Z')`);
    assert.equal(await evaluate("document.activeElement === window.imeField && window.imeField.isConnected"), true,
        "rendering preedit must preserve the browser's composing control");
    await send("Input.insertText", {text: "日本語"});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('A日本語Z')`);
    assert.equal(await evaluate("window.imeField.value"), "A日本語Z", "commit must replace preedit exactly once");
    assert.equal(await evaluate("window.imeEvents.filter(e => ['compositionstart', 'compositionupdate'].includes(e.type)).every(e => e.field && e.trusted)"), true,
        JSON.stringify(await evaluate("window.imeEvents")));
    assert.equal(await evaluate("window.imeEvents.some(e => e.type === 'compositionend' && e.field)"), true);
    await evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
    assert.deepEqual(await evaluate("[window.imeField.value, window.imeField.selectionStart, window.imeField.selectionEnd]"),
        ["A日本語Z", 4, 4], JSON.stringify(await evaluate("window.imeEvents")));

    await send("Input.imeSetComposition", {text: "消", selectionStart: 1, selectionEnd: 1});
    assert.equal(await evaluate("window.imeField.value"), "A日本語消Z", JSON.stringify(await evaluate("window.imeEvents")));
    await send("Input.imeSetComposition", {text: "", selectionStart: 0, selectionEnd: 0});
    await until("window.imeField.value === 'A日本語Z'");
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('A日本語Z')`);
    report.push("one accessible field owns trusted IME updates, Unicode replacement, commit, and cancellation");

    assert.equal(await evaluate("window.imeField.tagName"), "TEXTAREA", "multiline capability must not depend on the current value");
    await evaluate("window.imeField.select()");
    await send("Input.insertText", {text: "first\nsecond"});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('first\\nsecond')`);
    assert.equal(await evaluate("document.activeElement === window.imeField && window.imeField.value === 'first\\nsecond'"), true);
    await send("Input.dispatchKeyEvent", {type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9});
    await send("Input.dispatchKeyEvent", {type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9});
    await until(`document.activeElement?.getAttribute('aria-label') === 'Empty text field'`);
    await send("Input.insertText", {text: "second field"});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('Field 2 value: "second field"')`);
    assert.equal(await evaluate("window.imeField.value"), "first\nsecond");
    report.push("multiline edits preserve control identity and Tab transfers the input session");
    await send("Input.imeSetComposition", {text: "移", selectionStart: 1, selectionEnd: 1});
    await evaluate("window.imeField.focus()");
    await until(`!document.querySelector('[data-cranpose-composition]')`);
    await evaluate(`document.querySelector('[aria-label="Empty text field"]').focus()`);
    await send("Input.insertText", {text: "!"});
    await until(`document.querySelector('[data-cranpose-accessibility]').textContent.includes('second field移!')`);
    report.push("blurring during composition finishes the old session and permits further editing");

    await evaluate(`(() => {
        if (!document.querySelector('[data-cranpose-node][aria-label="Counter App"]')) {
            document.querySelector('[role="combobox"]').click();
        }
    })()`);
    await until(`!!document.querySelector('[data-cranpose-node][aria-label="Counter App"]')`);
    await evaluate(`document.querySelector('[data-cranpose-node][aria-label="Counter App"]').click()`);
    await until(`!!document.querySelector('[data-cranpose-node][aria-label="Increment"]')`);
    assert.equal(await evaluate("!window.imeField.isConnected && document.activeElement !== window.imeField"), true);
    report.push("removing the editor releases browser focus");

    inputUrl.searchParams.set("tab", "accessibility_robot");
    await send("Page.navigate", {url: inputUrl.href});
    await until(`!!document.querySelector('[aria-label="Passphrase"]')`);
    assert.equal(await evaluate(`document.querySelector('[aria-label="Passphrase"]').value`),
        "robot-secret-value", "an unfocused native password editor must reflect its application value");
    await evaluate(`document.querySelector('[aria-label="Notes"]').focus()`);
    await send("Input.dispatchKeyEvent", {type: "keyDown", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9});
    await send("Input.dispatchKeyEvent", {type: "keyUp", key: "Tab", code: "Tab", windowsVirtualKeyCode: 9});
    await until(`document.activeElement?.getAttribute('aria-label') === 'Passphrase'`);
    assert.equal(await evaluate("document.activeElement.type"), "password");
    assert.equal(await evaluate("document.activeElement.value"), "robot-secret-value");
    await evaluate("document.activeElement.select()");
    await send("Input.insertText", {text: "private🌍"});
    await until("document.activeElement.value === 'private🌍'");
    await evaluate(`new Promise(resolve => {
        const deferredSelection = event => event.stopImmediatePropagation();
        document.addEventListener('selectionchange', deferredSelection, true);
        document.activeElement.setSelectionRange(7, 9, 'backward');
        requestAnimationFrame(() => requestAnimationFrame(() => {
            document.removeEventListener('selectionchange', deferredSelection, true);
            resolve();
        }));
    })`);
    assert.deepEqual(await evaluate("[document.activeElement.selectionStart, document.activeElement.selectionEnd, document.activeElement.selectionDirection]"), [7, 9, "backward"]);
    await evaluate("document.dispatchEvent(new Event('selectionchange'))");
    await evaluate(`(() => {
        window.nativePasswordReference = document.createElement('input');
        window.nativePasswordReference.type = 'password';
        window.nativePasswordReference.setAttribute('aria-label', 'Native password reference');
        window.nativePasswordReference.value = 'private🌍';
        document.body.append(window.nativePasswordReference);
    })()`);
    const passwordTree = await send("Accessibility.getFullAXTree");
    const password = passwordTree.nodes.find(node => !node.ignored && node.name?.value === "Passphrase");
    const reference = passwordTree.nodes.find(node => !node.ignored && node.name?.value === "Native password reference");
    assert.ok(password && reference, "both native password controls are accessible");
    assert.deepEqual(password.value, reference.value, "password values must follow the browser's native protection policy");
    assert.ok(!JSON.stringify(passwordTree.nodes.filter(node => node !== password && node !== reference)).includes("private🌍"),
        "passwords must not leak into unrelated accessible nodes");
    await evaluate("window.nativePasswordReference.remove()");
    await evaluate(`document.querySelector('[aria-label="Notes"]').focus()`);
    await evaluate(`document.querySelector('[aria-label="Passphrase"]').focus()`);
    assert.equal(await evaluate("document.activeElement.value"), "private🌍", "password input must persist in application state");
    report.push("passwords use a native protected editor and retain their value across focus changes");
}