import {test} from 'node:test';
import assert from 'node:assert/strict';
import {files, pickFolder} from '../../src/file_picker/web_folder.mjs';

test('the first file is available while a later directory is still pending', async () => {
    let nestedVisited = false;
    const original = {name: 'first.mp3', arrayBuffer() { throw Error('must not read or copy audio'); }};
    const root = {name: 'music', async *values() {
        yield {kind: 'file', getFile: async () => original};
        yield {kind: 'directory', async *values() {
            nestedVisited = true;
            await new Promise(() => {});
        }};
    }};
    globalThis.window = {showDirectoryPicker: async () => root};
    const stream = files(await pickFolder());
    assert.equal((await stream.next()).value, original);
    assert.equal(nestedVisited, false);
});

test('walks nested directories and empty directories without losing files', async () => {
    const song = {name: 'song.flac'};
    const root = {async *values() {
        yield {kind: 'directory', async *values() {}};
        yield {kind: 'directory', async *values() {
            yield {kind: 'file', getFile: async () => song};
        }};
    }};
    const found = [];
    for await (const file of files(root)) found.push(file);
    assert.deepEqual(found, [song]);
});

test('cancel returns no folder, access failures remain errors', async () => {
    globalThis.window = {showDirectoryPicker: async () => { throw {name: 'AbortError'}; }};
    assert.equal(await pickFolder(), null);
    globalThis.window.showDirectoryPicker = async () => { throw Error('denied'); };
    await assert.rejects(pickFolder(), /denied/);
});
