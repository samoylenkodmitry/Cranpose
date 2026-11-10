// The chat beside the patchwork as a web page (`../../README.md`): an
// agent's answers arrive from a data worker as tokens and the page shows
// them as they come.
//
// In the web platform's idiom a token appends to the page only what it
// adds: characters to the last text node, a row to the table, an element
// for a new block. The list is a column laid out from its end, so it keeps
// to its end without a script, and it drops the blocks of turns that
// scrolled away. The picture is an `<img>`, which the browser animates. The
// status line's spinner turns by a transform in a layer of its own, and its
// text shows through a CSS gradient whose place the frame sets.

import { STATUS, TOKEN_CHARS, TOKENS_PER_FRAME, TURN, Token } from './chat-data.js';
import { element } from './wall.js';

// The tokens the worker may send ahead of the page.
const AHEAD = TOKENS_PER_FRAME * 4;
// The blocks the list keeps: those of the last two turns.
const HELD = 2 * TURN.length;
const SPIN_DEGREES = 12;
const SHIMMER_STEP = 6;
const SHIMMER_SPAN = 120;
const KEYWORDS = new Set(['fn', 'let', 'mut', 'if', 'return', 'usize', 'f32', 'Row']);
const SPINNER =
  '<svg class="chat-spinner" viewBox="0 0 24 24" fill="none" stroke-width="3">' +
  '<path d="M12 3 C16.97 3 21 7.03 21 12 C21 16.97 16.97 21 12 21 C7.03 21 3 16.97 3 12 C3 7.03 7.03 3 12 3 Z" stroke="#3B4A7A"/>' +
  '<path d="M12 3 C16.97 3 21 7.03 21 12" stroke="#14B8A6" stroke-linecap="round"/>' +
  '<path d="M21 12 C21 16.97 16.97 21 12 21" stroke="#3B82F6" stroke-linecap="round"/></svg>';

// One colored stretch of code: where it starts and ends, and its class.
type Span = readonly [start: number, end: number, kind: string];

function codeSpans(code: string): Span[] {
  const spans: Span[] = [];
  const pattern = /\/\/[^\n]*|"[^"]*"|[A-Za-z_][A-Za-z0-9_]*|[0-9][0-9.]*/g;
  for (const match of code.matchAll(pattern)) {
    const text = match[0];
    const kind = text.startsWith('//')
      ? 'comment'
      : text.startsWith('"')
        ? 'text'
        : /^[0-9]/.test(text)
          ? 'number'
          : KEYWORDS.has(text)
            ? 'keyword'
            : '';
    if (kind) {
      spans.push([match.index, match.index + text.length, kind]);
    }
  }
  return spans;
}

const CODE_SPANS = new Map<string, Span[]>();

// What a block shows `shown` tokens of.
type Grow = (shown: number) => void;

function turnLabel(turn: number): string {
  return `#${String(turn % 100000).padStart(5, '0')}`;
}

// Text that grows by the characters each token adds, appended to one node.
function typed(parent: HTMLElement, head: string, text: string): Grow {
  const node = document.createTextNode(head);
  parent.append(node);
  let chars = 0;
  return (shown) => {
    const next = Math.min(shown * TOKEN_CHARS, text.length);
    node.appendData(text.slice(chars, next));
    chars = next;
  };
}

// Code that grows the same way: each character goes to the end of the
// colored stretch it belongs to, or of plain text between two of them.
function code(parent: HTMLElement, turn: number, text: string): Grow {
  let spans = CODE_SPANS.get(text);
  if (!spans) {
    spans = codeSpans(text);
    CODE_SPANS.set(text, spans);
  }
  element('span', 'code-comment', parent).textContent = `// ${turnLabel(turn)}\n`;
  let chars = 0;
  let span = 0;
  let node: Text | null = null;
  let nodeEnd = 0;
  return (shown) => {
    const next = Math.min(shown * TOKEN_CHARS, text.length);
    while (chars < next) {
      if (!node || chars >= nodeEnd) {
        while (span < spans.length && spans[span][1] <= chars) {
          span += 1;
        }
        const inside = span < spans.length && spans[span][0] <= chars;
        nodeEnd = inside ? spans[span][1] : span < spans.length ? spans[span][0] : text.length;
        node = document.createTextNode('');
        if (inside) {
          element('span', `code-${spans[span][2]}`, parent).append(node);
        } else {
          parent.append(node);
        }
      }
      const end = Math.min(next, nodeEnd);
      node.appendData(text.slice(chars, end));
      chars = end;
    }
  };
}

function table(parent: HTMLElement, rows: readonly (readonly string[])[]): Grow {
  const body = element('table', 'chat-table', parent);
  let count = 0;
  return (shown) => {
    for (const end = Math.min(shown, rows.length); count < end; count++) {
      const row = element('tr', '', body);
      for (const cell of rows[count]) {
        element(count === 0 ? 'th' : 'td', '', row).textContent = cell;
      }
    }
  };
}

// Builds block `index` at the end of `list`.
function build(list: HTMLElement, index: number): Grow {
  const block = TURN[index % TURN.length];
  const turn = Math.trunc(index / TURN.length);
  switch (block.kind) {
    case 'user':
      element('div', 'chat-user', list).textContent = block.text;
      return () => {};
    case 'paragraph':
      return typed(element('div', 'chat-paragraph', list), `${turnLabel(turn)} `, block.text);
    case 'code':
      return code(element('pre', 'chat-code', list), turn, block.text);
    case 'table':
      return table(list, block.rows);
    case 'gif': {
      const picture = element('img', 'chat-gif', list);
      picture.src = 'chat-assets/spin.gif';
      return () => {};
    }
    case 'buttons': {
      const row = element('div', 'chat-buttons', list);
      for (const label of block.labels) {
        element('button', '', row).textContent = label;
      }
      return () => {};
    }
  }
}

/** Builds the chat in `parent` and starts its data worker. Returns what
 * each frame does: it takes the frame's tokens and sets the status line. */
export function chat(parent: HTMLElement): (frame: number) => void {
  const pane = element('div', 'chat', parent);
  const list = element('div', 'chat-list', element('div', 'chat-scroll', pane));
  const status = element('div', 'chat-status', pane);
  status.innerHTML = SPINNER;
  const spinner = status.firstElementChild as SVGElement;
  const said = document.createTextNode('');
  const shimmer = element('span', 'chat-shimmer', status);
  shimmer.append(said);

  const arrived: Token[] = [];
  const worker = new Worker(new URL('./chat-worker.js', import.meta.url), { type: 'module' });
  worker.onmessage = (event: MessageEvent<Token>) => arrived.push(event.data);
  worker.postMessage(AHEAD);

  let blocks = 0;
  let grow: Grow = () => {};
  let last: Token | undefined;
  return (frame) => {
    const taken = arrived.splice(0, TOKENS_PER_FRAME);
    for (const token of taken) {
      const [, block, shown] = token;
      while (blocks <= block) {
        grow = build(list, blocks);
        blocks += 1;
        if (list.childElementCount > HELD) {
          list.firstElementChild!.remove();
        }
      }
      grow(shown);
      last = token;
    }
    if (taken.length > 0) {
      worker.postMessage(taken.length);
    }
    spinner.style.transform = `rotate(${(frame * SPIN_DEGREES) % 360}deg)`;
    shimmer.style.backgroundPositionX = `${(frame * SHIMMER_STEP) % SHIMMER_SPAN}px`;
    if (last) {
      const kind = TURN[last[1] % TURN.length].kind;
      said.data = `${STATUS[kind]}… (${String(last[0] % 1000000).padStart(6, '0')} tokens)`;
    }
  };
}
