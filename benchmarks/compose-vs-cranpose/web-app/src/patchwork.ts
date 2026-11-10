// The patchwork as a web page (`../../README.md`): nine patches of the still
// wall's tiles. Two never change; in the others a frame changes a small
// part. Its data follows `perf-data/src/patchwork.rs` bit for bit.
//
// In the web platform's idiom the page builds each patch's elements once and
// each frame touches only what changed: the backgrounds, the cells and the
// text nodes of the eighth of a patch whose turn it is, the two items that
// change places, the paragraph's text, two tiles of the pile. The churn
// patch keeps each cell's three kinds of content and puts the new one in
// the cell. The scroll patch moves the row that left to the list's end and
// writes the row that came in into it, as a list that recycles its rows
// does.

import { PALETTE } from '../../shared-ts/data.js';
import { chat } from './chat.js';
import { GAP, TIERS, clock, dotColor, element, fit, label, tile, tileSizes, value } from './wall.js';

const DEPTH = 12;
const CHART_POINTS = 512;
const JUMP = 37;
const PILE_ALPHA = '99';
const PILE_OPAQUE = '#000000';

function reorderUnits(item: number): number {
  return ((Math.imul(item, 2654435761) >>> 22) % 3) + 1;
}

function paragraph(frame: number, words: number): string {
  const changed = (frame * JUMP) % Math.max(words, 1);
  const text: string[] = [];
  for (let word = 0; word < words; word++) {
    text.push(label(word === changed ? frame % 100000 : word));
  }
  return text.join(' ');
}

// One patch: what a frame changes in it.
type Show = (frame: number) => void;

/** The changes of item `index` up to frame `frame` in a patch of `columns`
 * by `rows` items that change in turn: one more every `rows` frames, in the
 * frame of the item's row. */
function step(index: number, frame: number, columns: number, rows: number): number {
  return Math.trunc((Math.trunc(index / columns) + frame) / rows);
}

// A patch of `columns` by `rows` items that change in turn: `change` runs
// for the items of the row whose turn frame `frame` is, or for all of them
// when the frame does not follow the one shown before.
function inTurn(
  columns: number,
  rows: number,
  change: (index: number, changes: number) => void,
): Show {
  let shown = -2;
  return (frame) => {
    const row = (rows - (frame % rows)) % rows;
    const [from, to] = frame === shown + 1 ? [row * columns, (row + 1) * columns] : [0, columns * rows];
    for (let index = from; index < to; index++) {
      change(index, step(index, frame, columns, rows));
    }
    shown = frame;
  };
}

function patch(area: HTMLElement, className: string): HTMLDivElement {
  return element('div', `patch ${className}`, area);
}

function still(area: HTMLElement, tiles: number) {
  const grid = patch(area, 'patch-grid');
  for (let index = 0; index < tiles; index++) {
    tile(index, grid);
  }
}

// Each chart is an SVG line the page builds once.
function charts(area: HTMLElement, columns: number, rows: number, width: number, height: number) {
  const grid = patch(area, 'patch-grid patch-pairs');
  const pairs = Math.max(1, Math.trunc(columns / 2));
  grid.style.setProperty('--pairs', String(pairs));
  const count = pairs * Math.max(1, Math.trunc(rows / 2));
  const [chartWidth, chartHeight] = [2 * width + GAP, 2 * height + GAP];
  const step = chartWidth / (CHART_POINTS - 1);
  const svg = 'http://www.w3.org/2000/svg';
  for (let index = 0; index < count; index++) {
    const points: string[] = [];
    for (let point = 0; point < CHART_POINTS; point++) {
      const at = point * 0.125;
      const value =
        0.5 + 0.38 * Math.sin(at * 0.32 + index * 0.77) + 0.08 * Math.sin(at * 1.7 + index);
      points.push(`${point * step},${chartHeight * (1 - value)}`);
    }
    const chart = document.createElementNS(svg, 'svg');
    chart.setAttribute('class', 'patch-chart');
    const line = document.createElementNS(svg, 'polyline');
    line.setAttribute('points', points.join(' '));
    line.setAttribute('stroke', PALETTE[index % PALETTE.length]);
    chart.append(line);
    grid.append(chart);
  }
}

function recolor(area: HTMLElement, columns: number, rows: number): Show {
  const tiles = columns * rows;
  const grid = patch(area, 'patch-grid');
  const built = Array.from({ length: tiles }, (_, index) => tile(index, grid));
  return inTurn(columns, rows, (index, changes) => {
    built[index].style.background = PALETTE[(index + changes) % PALETTE.length];
  });
}

function churn(area: HTMLElement, columns: number, rows: number): Show {
  const tiles = columns * rows;
  const grid = patch(area, 'patch-grid');
  const kinds: HTMLDivElement[][] = [];
  const shown: HTMLDivElement[] = [];
  const spare = document.createDocumentFragment();
  for (let index = 0; index < tiles; index++) {
    const whole = tile(index, grid);
    const blank = element('div', 'patch-blank', spare);
    blank.style.background = dotColor(index);
    const named = element('div', 'wall-tile', spare);
    named.style.background = PALETTE[index % PALETTE.length];
    element('div', 'wall-label', named).textContent = label(index);
    kinds.push([whole, blank, named]);
    shown.push(whole);
  }
  spare.replaceChildren();
  return inTurn(columns, rows, (index, changes) => {
    const next = kinds[index][changes % 3];
    if (next !== shown[index]) {
      shown[index].replaceWith(next);
      shown[index] = next;
    }
  });
}

function reorder(area: HTMLElement, tiles: number, width: number): Show {
  const row = patch(area, 'patch-flow');
  const count = Math.max(1, Math.trunc((tiles * 2) / 5));
  const items: HTMLDivElement[] = [];
  for (let item = 0; item < count; item++) {
    const units = reorderUnits(item);
    const built = tile(item, row);
    built.style.width = `${units * width + (units - 1) * GAP}px`;
    items.push(built);
  }
  // The first of the two places whose items changed places, or none yet.
  let swapped = -1;
  return (frame) => {
    if (count < 2) {
      return;
    }
    if (swapped >= 0) {
      row.insertBefore(items[swapped], items[swapped + 1]);
    }
    swapped = (frame * JUMP) % (count - 1);
    row.insertBefore(items[swapped + 1], items[swapped]);
  };
}

function deep(area: HTMLElement, columns: number, rows: number): Show {
  const grid = patch(area, 'patch-grid patch-pairs');
  const pairs = Math.max(1, Math.trunc(columns / 2));
  grid.style.setProperty('--pairs', String(pairs));
  const down = Math.max(1, Math.trunc(rows / 2));
  const count = pairs * down;
  const values: Text[] = [];
  for (let index = 0; index < count; index++) {
    let box: ParentNode = grid;
    for (let level = 0; level < DEPTH; level++) {
      const next = element('div', 'patch-nest', box);
      next.style.background = PALETTE[(index + level) % PALETTE.length];
      box = next;
    }
    const text = document.createTextNode('');
    box.append(text);
    values.push(text);
  }
  return inTurn(pairs, down, (index, changes) => {
    values[index].data = String((index * 7 + changes) % 1000).padStart(3, '0');
  });
}

// The list as a scroll container, which the page scrolls by a row each
// frame. Its sheet is as tall as the whole list and holds the rows a patch
// shows and the one its edge cuts; the row that left is placed under the last one and takes the tiles
// of the row that came in, as a list that recycles its rows does.
function scroll(area: HTMLElement, columns: number, rows: number, height: number): Show {
  const scroller = patch(area, 'patch-scroller');
  const sheet = element('div', 'patch-sheet', scroller);
  const total = Math.trunc(100000 / columns);
  const pitch = height + GAP;
  sheet.style.height = `${total * pitch}px`;
  const built = Array.from({ length: rows + 1 }, () => {
    const row = element('div', 'patch-row', sheet);
    const tiles = Array.from({ length: columns }, (_, column) => {
      const whole = tile(column, row);
      const line = whole.lastElementChild!;
      return {
        whole,
        label: whole.firstElementChild!.firstChild as Text,
        dot: line.firstElementChild as HTMLElement,
        value: line.lastElementChild!.firstChild as Text,
      };
    });
    return { row, tiles };
  });
  // Puts `shown` at row `at` of the list with that row's tiles.
  const write = (shown: (typeof built)[number], at: number) => {
    shown.row.style.top = `${at * pitch}px`;
    shown.tiles.forEach((written, column) => {
      const index = at * columns + column;
      written.whole.style.background = PALETTE[index % PALETTE.length];
      written.label.data = label(index);
      written.dot.style.background = dotColor(index);
      written.value.data = value(index);
    });
  };
  let first = -2;
  return (frame) => {
    const next = frame % (total - rows + 1);
    if (next === first + 1) {
      const left = built.shift()!;
      built.push(left);
      write(left, next + rows);
    } else {
      built.forEach((shown, place) => write(shown, next + place));
    }
    scroller.scrollTop = next * pitch;
    first = next;
  };
}

function paragraphPatch(area: HTMLElement, tiles: number): Show {
  const text = document.createTextNode('');
  patch(area, 'patch-paragraph').append(text);
  return (frame) => {
    text.data = paragraph(frame, tiles * 3);
  };
}

function pile(area: HTMLElement, columns: number, rows: number, width: number, height: number): Show {
  const box = patch(area, 'patch-pile');
  const across = Math.max(1, columns - 1);
  const count = across * Math.max(1, rows - 1);
  const built = Array.from({ length: count }, (_, index) => {
    const whole = tile(index, box);
    whole.style.left = `${GAP + (index % across) * (width + GAP)}px`;
    whole.style.top = `${GAP + Math.trunc(index / across) * (height + GAP)}px`;
    return whole;
  });
  const fill = (index: number) => `${PALETTE[index % PALETTE.length]}${PILE_ALPHA}`;
  built.forEach((whole, index) => (whole.style.background = fill(index)));
  let opaque = -1;
  return (frame) => {
    if (opaque >= 0) {
      built[opaque].style.background = fill(opaque);
    }
    opaque = (frame * JUMP) % count;
    built[opaque].style.background = PILE_OPAQUE;
  };
}

/** Shows the patchwork of `tier` in place of the page's gauntlet, and
 * advances its frame each animation frame until frame `freeze`. `only` 1 to
 * 9 shows that patch alone, 10 the chat alone, and 0 everything. */
export function patchwork(
  tier: number,
  freeze: number,
  log: (message: string) => void,
  only = 0,
) {
  const [width, height] = TIERS[Math.min(Math.max(tier, 1), TIERS.length) - 1];
  document.title = 'Patchwork';
  document.body.replaceChildren();
  document.body.classList.add('still-wall');
  element('header', 'top-bar', document.body).textContent = 'Patchwork';
  const line = element('div', 'wall-clock', document.body);
  const screen = element('div', 'patchwork-screen', document.body);
  const area = element('div', 'patchwork', screen);
  const showChat = only === 0 || only === 10 ? chat(screen) : () => {};
  tileSizes(area, width, height);
  let columns = 0;
  let rows = 0;
  let frame = 0;
  let patches: Show[] = [];
  const show = () => {
    line.textContent = clock(frame);
    showChat(frame);
    for (const patchShow of patches) {
      patchShow(frame);
    }
  };
  // Builds the patches for the tiles that fit in one, again only when that
  // number changes.
  const lay = () => {
    const patchWidth = Math.floor(area.clientWidth / 3);
    const patchHeight = Math.floor(area.clientHeight / 3);
    const across = fit(patchWidth, width);
    const down = fit(patchHeight, height);
    if (across === columns && down === rows) {
      return;
    }
    columns = across;
    rows = down;
    const tiles = columns * rows;
    area.style.setProperty('--patch-width', `${patchWidth}px`);
    area.style.setProperty('--patch-height', `${patchHeight}px`);
    area.style.setProperty('--columns', String(columns));
    area.replaceChildren();
    const builders: (() => Show | void)[] = [
      () => still(area, tiles),
      () => charts(area, columns, rows, width, height),
      () => recolor(area, columns, rows),
      () => churn(area, columns, rows),
      () => reorder(area, tiles, width),
      () => deep(area, columns, rows),
      () => scroll(area, columns, rows, height),
      () => paragraphPatch(area, tiles),
      () => pile(area, columns, rows, width, height),
    ];
    patches = [];
    builders.forEach((build, place) => {
      if (only === 0 || only === place + 1) {
        const shown = build();
        if (shown) {
          patches.push(shown);
        }
      } else {
        patch(area, '');
      }
    });
    show();
  };
  lay();
  new ResizeObserver(lay).observe(area);
  const tick = () => {
    frame += 1;
    show();
    if (freeze > 0 && frame >= freeze) {
      log(`PERF frozen frame=${frame}`);
      return;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
  requestAnimationFrame(() => log('PERF first_frame'));
}
