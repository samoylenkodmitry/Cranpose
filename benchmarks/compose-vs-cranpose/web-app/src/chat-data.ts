// The chat's turn and its tokens (`../../README.md`), which the page and its
// data worker share. They follow `perf-data/src/chat.rs` bit for bit.

export const TOKEN_CHARS = 6;
export const TOKENS_PER_FRAME = 4;

export type Block =
  | { kind: 'user' | 'heading' | 'paragraph' | 'code'; text: string }
  | { kind: 'table'; rows: readonly (readonly string[])[] }
  | { kind: 'gif' }
  | { kind: 'buttons'; labels: readonly string[] };

export const TURN: readonly Block[] = [
  { kind: 'user', text: 'Why does the list stutter when one row changes?' },
  { kind: 'heading', text: 'What happens in a frame' },
  {
    kind: 'paragraph',
    text:
      'The list stutters because a small change starts a large amount of work. The row ' +
      'that changed shows a counter, and the counter gets a new value on every frame. ' +
      'The row measures its text again, which is right, because the text is new. The ' +
      'row then tells its parent that its size may have changed. The parent does not ' +
      'check whether the size really changed. It measures and places every one of its ' +
      'children again, the ones on the screen and the ones kept ready beyond its edges. ' +
      'Each child measures its own text again, although none of that text changed. A ' +
      'frame that should cost one row costs the whole list, and the cost grows with ' +
      'the number of rows, not with the size of the change.',
  },
  {
    kind: 'paragraph',
    text:
      'The profile shows the same thing from the other side. Most of the frame goes to ' +
      'text shaping, and nearly all of the shaped strings are equal to the strings of ' +
      'the frame before. The cache of shaped text does not help, because the list asks ' +
      'for each string at a width that differs by a fraction of a point from the width ' +
      'of the last frame, so every lookup misses. Drawing takes a small part of the ' +
      'frame and does not change, because the picture is almost the same.',
  },
  { kind: 'heading', text: 'Three things to change' },
  {
    kind: 'paragraph',
    text:
      '- Give the row a fixed height. A row whose size cannot change needs no new ' +
      'layout of its parent, so the work stops at the row.',
  },
  {
    kind: 'paragraph',
    text:
      '- Round the width that the list passes to its rows to whole points. Equal ' +
      'strings then meet equal widths, and the cache of shaped text answers.',
  },
  {
    kind: 'paragraph',
    text:
      '- Read the counter in the draw phase where the toolkit allows it. A value ' +
      'that changes only what is drawn should not start composition or layout.',
  },
  {
    kind: 'code',
    text:
      '// Measure only the row that changed.\n' +
      'fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n' +
      '    let row = &mut rows[dirty];\n' +
      '    if row.width == 320.0 {\n' +
      '        return row.height;\n' +
      '    }\n' +
      '    row.label = "measured";\n' +
      '    row.height = row.measure(320.0);\n' +
      '    row.height\n' +
      '}\n' +
      '\n' +
      '// A row of fixed height ends the pass.\n' +
      'fn place(rows: &mut [Row], first: usize) {\n' +
      '    let mut top = 0.0;\n' +
      '    for row in &mut rows[first..] {\n' +
      '        row.top = top;\n' +
      '        top = top + row.height;\n' +
      '    }\n' +
      '}\n',
  },
  {
    kind: 'paragraph',
    text:
      'With the boundary in place the frame lays out one row and leaves the others ' +
      'alone. I ran the list of two thousand rows before and after the change, on the ' +
      'same device and at the same scroll position. These are the passes of one frame, ' +
      'as the median of three hundred frames:',
  },
  {
    kind: 'table',
    rows: [
      ['Pass', 'Before', 'After', 'Change'],
      ['Compose', '1.92 ms', '0.31 ms', '-84%'],
      ['Measure', '4.70 ms', '0.22 ms', '-95%'],
      ['Place', '1.18 ms', '0.09 ms', '-92%'],
      ['Draw', '2.46 ms', '2.41 ms', '-2%'],
      ['Frame', '10.26 ms', '3.03 ms', '-70%'],
    ],
  },
  { kind: 'gif' },
  {
    kind: 'paragraph',
    text:
      'Drawing is now the largest pass, and it did not move. The next step is to keep ' +
      'the drawing of the rows that did not change and to draw only the row with the ' +
      'counter. That needs a look at how the list records its rows, which I have not ' +
      'read yet. The picture above turns while the benchmark runs; it stops when the ' +
      'run ends. Tell me whether to apply the three changes, or to show the diff first.',
  },
  { kind: 'buttons', labels: ['Apply', 'Show diff', 'Dismiss'] },
  { kind: 'paragraph', text: 'Waiting for your answer.' },
];

/** The tokens `block` takes to appear whole. */
export function tokens(block: Block): number {
  switch (block.kind) {
    case 'heading':
    case 'paragraph':
    case 'code':
      return Math.ceil(block.text.length / TOKEN_CHARS);
    case 'table':
      return block.rows.length;
    default:
      return 1;
  }
}

/** What the status line says the agent does while a block of `kind` is the
 * last one. */
export const STATUS: Record<Block['kind'], string> = {
  user: 'Reading',
  heading: 'Writing',
  paragraph: 'Writing',
  code: 'Writing code',
  table: 'Filling the table',
  gif: 'Drawing',
  buttons: 'Waiting',
};

/** A token: its number from 1, the chat's last block and the tokens of that
 * block that arrived. */
export type Token = readonly [count: number, block: number, shown: number];

const STARTS: number[] = [];
let turnTokens = 0;
for (const block of TURN) {
  STARTS.push(turnTokens);
  turnTokens += tokens(block);
}

/** The chat after `count` tokens. */
export function tokenAfter(count: number): Token {
  const index = Math.max(count, 1) - 1;
  const turn = Math.trunc(index / turnTokens);
  const within = index % turnTokens;
  let block = TURN.length - 1;
  while (STARTS[block] > within) {
    block -= 1;
  }
  return [count, turn * TURN.length + block, within - STARTS[block] + 1];
}
