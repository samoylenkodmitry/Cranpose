export async function pickFolder() {
    if (typeof window.showDirectoryPicker === 'function') {
        try {
            return await window.showDirectoryPicker({mode: 'read'});
        } catch (error) {
            if (error.name === 'AbortError') return null;
            throw error;
        }
    }
    // Browsers without directory handles enumerate before delivering FileList.
    return new Promise(resolve => {
        const input = document.createElement('input');
        input.type = 'file';
        input.webkitdirectory = true;
        input.multiple = true;
        input.hidden = true;
        const finish = result => { input.remove(); resolve(result); };
        input.oncancel = () => finish(null);
        input.onchange = () => {
            const files = input.files;
            finish(files.length ? {
                name: files[0].webkitRelativePath.split('/')[0],
                files,
            } : null);
        };
        document.body.append(input);
        input.click();
    });
}

export function folderName(folder) { return folder.name; }
export function isFolder(entry) { return entry.kind === 'directory'; }
export function entries(folder) {
    return folder.files ? folder.files[Symbol.iterator]() : folder.values();
}
export async function nextEntry(iterator) {
    const {done, value} = await iterator.next();
    if (done) return null;
    return value.kind === 'file' ? value.getFile() : value;
}
export async function* files(folder) {
    const stack = [entries(folder)];
    while (stack.length) {
        const entry = await nextEntry(stack[stack.length - 1]);
        if (entry === null) stack.pop();
        else if (isFolder(entry)) stack.push(entries(entry));
        else yield entry;
    }
}
