// The patchwork as a web page (`../../README.md`): nine patches of the still
// wall's tiles, each of which changes in a way of its own every frame. Its
// data follows `perf-data/src/patchwork.rs` bit for bit.
//
// In the web platform's idiom the page builds each patch's elements once and
// each frame sets only what changed: the recolor patch's backgrounds, the
// nests' text nodes, the paragraph's text. The churn patch keeps each cell's
// three kinds of content and puts the frame's one in the cell. The reorder
// patch moves its first item to the end of its wrapping row. The jump patch
// keeps the rows a patch shows and writes the frame's rows into them, as a
// list that recycles its rows does.

import { PALETTE } from '../../shared-ts/data.js';
import { GAP, TIERS, clock, dotColor, element, fit, label, tile, tileSizes, value } from './wall.js';

/** The width of the chat at the right of the screen. */
const CHAT_WIDTH = 320;
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

function recolor(area: HTMLElement, tiles: number): Show {
  const grid = patch(area, 'patch-grid');
  const built = Array.from({ length: tiles }, (_, index) => tile(index, grid));
  return (frame) => {
    for (let index = 0; index < tiles; index++) {
      built[index].style.background = PALETTE[(index + frame) % PALETTE.length];
    }
  };
}

function churn(area: HTMLElement, tiles: number): Show {
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
  return (frame) => {
    for (let index = 0; index < tiles; index++) {
      const next = kinds[index][(index + frame) % 3];
      shown[index].replaceWith(next);
      shown[index] = next;
    }
  };
}

function reorder(area: HTMLElement, tiles: number, width: number): Show {
  const row = patch(area, 'patch-flow');
  const count = Math.max(1, Math.trunc((tiles * 2) / 5));
  for (let item = 0; item < count; item++) {
    const units = reorderUnits(item);
    tile(item, row).style.width = `${units * width + (units - 1) * GAP}px`;
  }
  let turned = 0;
  return (frame) => {
    for (; turned < frame; turned++) {
      row.append(row.firstChild!);
    }
  };
}

function deep(area: HTMLElement, columns: number, rows: number): Show {
  const grid = patch(area, 'patch-grid patch-pairs');
  const pairs = Math.max(1, Math.trunc(columns / 2));
  grid.style.setProperty('--pairs', String(pairs));
  const count = pairs * Math.max(1, Math.trunc(rows / 2));
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
  return (frame) => {
    for (let index = 0; index < count; index++) {
      values[index].data = String((index * 7 + frame) % 1000).padStart(3, '0');
    }
  };
}

function jump(area: HTMLElement, columns: number, rows: number): Show {
  const grid = patch(area, 'patch-grid');
  const total = Math.trunc(100000 / columns);
  // The rows a patch shows and the one its edge cuts.
  const count = (rows + 1) * columns;
  const built = Array.from({ length: count }, (_, index) => {
    const whole = tile(index, grid);
    const row = whole.lastElementChild!;
    return {
      whole,
      label: whole.firstElementChild!.firstChild as Text,
      dot: row.firstElementChild as HTMLElement,
      value: row.lastElementChild!.firstChild as Text,
    };
  });
  return (frame) => {
    const first = ((frame * JUMP) % (total - rows + 1)) * columns;
    for (let place = 0; place < count; place++) {
      const index = first + place;
      const shown = built[place];
      shown.whole.style.background = PALETTE[index % PALETTE.length];
      shown.label.data = label(index);
      shown.dot.style.background = dotColor(index);
      shown.value.data = value(index);
    }
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
 * advances its frame each animation frame until frame `freeze`. */
export function patchwork(tier: number, freeze: number, log: (message: string) => void) {
  const [width, height] = TIERS[Math.min(Math.max(tier, 1), TIERS.length) - 1];
  document.title = 'Patchwork';
  document.body.replaceChildren();
  document.body.classList.add('still-wall');
  element('header', 'top-bar', document.body).textContent = 'Patchwork';
  const line = element('div', 'wall-clock', document.body);
  const area = element('div', 'patchwork', document.body);
  tileSizes(area, width, height);
  let columns = 0;
  let rows = 0;
  let frame = 0;
  let patches: Show[] = [];
  const show = () => {
    line.textContent = clock(frame);
    for (const patchShow of patches) {
      patchShow(frame);
    }
  };
  // Builds the patches for the tiles that fit in one, again only when that
  // number changes.
  const lay = () => {
    const patchWidth = Math.floor((area.clientWidth - CHAT_WIDTH) / 3);
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
    still(area, tiles);
    charts(area, columns, rows, width, height);
    patches = [
      recolor(area, tiles),
      churn(area, tiles),
      reorder(area, tiles, width),
      deep(area, columns, rows),
      jump(area, columns, rows),
      paragraphPatch(area, tiles),
      pile(area, columns, rows, width, height),
    ];
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
