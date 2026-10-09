// Presses the page's "Enable accessibility" button until the mirror is built.
// A Cranpose web app builds no mirror until a screen reader presses it.
export const ENABLE_MIRROR = `(() => {
    document.querySelector('[data-cranpose-enable-accessibility]')?.click();
    return !!document.querySelector('[data-cranpose-accessibility]');
})()`;

// Joins console arguments the way DevTools shows them: "%c" marks a style
// argument, which carries no text.
function consoleText(args) {
    const [format, ...rest] = args.map(arg => arg.value ?? arg.description ?? "");
    if (typeof format !== "string" || !format.includes("%c")) return [format, ...rest].join(" ");
    const styles = format.split("%c").length - 1;
    return [format.replaceAll("%c", ""), ...rest.slice(styles)].join(" ");
}

export class Cdp {
    constructor() {
        this.pending = new Map();
        this.nextId = 0;
        this.logs = [];
    }

    async connect(url) {
        this.ws = new WebSocket(url);
        await new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error("debugger connection timed out")), 10000);
            this.ws.onopen = () => { clearTimeout(timer); resolve(); };
            this.ws.onerror = error => { clearTimeout(timer); reject(error); };
        });
        this.ws.onmessage = event => {
            const message = JSON.parse(event.data);
            if (message.method === "Log.entryAdded") {
                this.logs.push(message.params.entry.level + ": " + message.params.entry.text);
            }
            if (message.method === "Runtime.consoleAPICalled") {
                this.logs.push(message.params.type + ": " + consoleText(message.params.args));
            }
            if (message.method === "Runtime.exceptionThrown") {
                const details = message.params.exceptionDetails;
                this.logs.push("exception: " + details.text + " " + (details.exception?.description ?? ""));
            }
            const pending = this.pending.get(message.id);
            if (!pending) return;
            this.pending.delete(message.id);
            clearTimeout(pending.timer);
            if (message.error) pending.reject(new Error(JSON.stringify(message.error)));
            else pending.resolve(message.result);
        };
    }

    send(method, params = {}) {
        const id = ++this.nextId;
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => {
                this.pending.delete(id);
                reject(new Error(method + " timed out"));
            }, 15000);
            this.pending.set(id, {resolve, reject, timer});
            this.ws.send(JSON.stringify({id, method, params}));
        });
    }

    async evaluate(expression) {
        const result = await this.send("Runtime.evaluate", {expression, returnByValue: true, awaitPromise: true});
        if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
        return result.result.value;
    }

    async until(expression) {
        for (let attempt = 0; attempt < 150; attempt++) {
            if (await this.evaluate(expression)) return;
            await new Promise(resolve => setTimeout(resolve, 200));
        }
        throw new Error("Timed out: " + expression);
    }

    close() {
        this.ws?.close();
        for (const pending of this.pending.values()) {
            clearTimeout(pending.timer);
            pending.reject(new Error("debugger connection closed"));
        }
        this.pending.clear();
    }
}
