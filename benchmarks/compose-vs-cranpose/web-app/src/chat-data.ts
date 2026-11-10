// The chat's turn and its tokens (`../../README.md`), which the page and its
// data worker share. They follow `perf-data/src/chat.rs` bit for bit.

export const TOKEN_CHARS = 6;
export const TOKENS_PER_FRAME = 4;

export type Block =
  | { kind: 'user' | 'paragraph' | 'code'; text: string }
  | { kind: 'table'; rows: readonly (readonly string[])[] }
  | { kind: 'gif' }
  | { kind: 'buttons'; labels: readonly string[] };

export const TURN: readonly Block[] = [
  { kind: 'user', text: 'Why does the list stutter when one row changes?' },
  {
    kind: 'paragraph',
    text:
      'The row measures its text again on every frame. Its parent then lays out all of ' +
      'its children, although only one of them changed size. A boundary around the row ' +
      'keeps the work inside it.',
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
      '}\n',
  },
  {
    kind: 'paragraph',
    text:
      'With the boundary the frame lays out one row, not the list. These are the passes ' +
      'before and after the change:',
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
  { kind: 'paragraph', text: 'The picture above turns while the benchmark runs.' },
  { kind: 'buttons', labels: ['Apply', 'Show diff', 'Dismiss'] },
];

/** The tokens `block` takes to appear whole. */
export function tokens(block: Block): number {
  switch (block.kind) {
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
