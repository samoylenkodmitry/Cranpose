// The chat's data layer: a worker, the thread a page has. It sends one
// message a token and no more tokens than the page asked for, so it waits
// while the page is behind, as a bounded channel makes a thread wait.

import { tokenAfter } from './chat-data.js';

const worker = self as unknown as {
  onmessage: ((event: MessageEvent<number>) => void) | null;
  postMessage(token: unknown): void;
};

let count = 0;
worker.onmessage = (event) => {
  for (let wanted = event.data; wanted > 0; wanted--) {
    count += 1;
    worker.postMessage(tokenAfter(count));
  }
};
