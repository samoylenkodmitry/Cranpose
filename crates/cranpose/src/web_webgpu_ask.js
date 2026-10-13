// Asks the browser for a WebGPU adapter when the page imports the app's
// module, before its WebAssembly downloads and compiles. A browser answers
// only after its GPU process has tried to make an adapter, and the page has
// nothing else to do while the module loads.
const present = (async () => {
    try {
        return Boolean(navigator.gpu) && (await navigator.gpu.requestAdapter()) !== null;
    } catch (error) {
        return false;
    }
})();

export function cranposeWebGpuPresent() {
    return present;
}
