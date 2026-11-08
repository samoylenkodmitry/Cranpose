// The still wall as a web page (`../../README.md`): a window of tiles that
// never change, under a clock line whose text changes every frame, with a
// cursor outline that rests on another tile every frame. Its data follows
// `perf-data/src/wall.rs` bit for bit.
//
// In the web platform's idiom the page builds the tiles' elements once, in a
// CSS grid. Each frame sets the clock's text and the cursor's transform; the
// cursor has a layer of its own (`will-change: transform`), as a graphics
// layer gives it in the other apps.

import { CHIP_BACKGROUND, PALETTE } from '../../shared-ts/data.js';

/** The tiles' width and height in points, tier 1 to 20. */
const TIERS: readonly (readonly [number, number])[] = [
  [64, 40], [54, 34], [46, 29], [40, 25], [34, 21], [30, 18], [26, 16], [22, 14], [19, 12], [16, 10],
  [14, 9], [12, 8], [10, 7], [8, 6], [7, 5], [6, 4], [5, 4], [5, 3], [4, 3], [3, 3],
];
const GAP = 1;
const CURSOR_WIDTH = 2;

function fit(space: number, tile: number): number {
  return Math.max(1, Math.floor((space - GAP) / (tile + GAP)));
}

function label(index: number): string {
  return String(index).padStart(5, '0');
}

function value(index: number): string {
  const mixed = Math.imul(index, 2654435761) >>> 22;
  return String(mixed % 1000).padStart(3, '0');
}

function cursor(frame: number, tiles: number): number {
  return (frame * 37) % Math.max(tiles, 1);
}

function clock(frame: number): string {
  return `frame ${String(frame).padStart(6, '0')}`;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, parent: ParentNode) {
  const created = document.createElement(tag);
  created.className = className;
  parent.append(created);
  return created;
}

/** Shows the still wall of `tier` in place of the page's gauntlet, and
 * advances its frame each animation frame until frame `freeze`. */
export function wall(tier: number, freeze: number, log: (message: string) => void) {
  const [width, height] = TIERS[Math.min(Math.max(tier, 1), TIERS.length) - 1];
  document.title = 'Still wall';
  document.body.replaceChildren();
  document.body.classList.add('still-wall');
  element('header', 'top-bar', document.body).textContent = 'Still wall';
  const line = element('div', 'wall-clock', document.body);
  const area = element('div', 'wall', document.body);
  const style = area.style;
  style.setProperty('--width', `${width}px`);
  style.setProperty('--height', `${height}px`);
  style.setProperty('--label', `${height * 0.3}px`);
  style.setProperty('--value', `${height * 0.26}px`);
  style.setProperty('--dot', `${height * 0.22}px`);
  style.setProperty('--pad', `${height * 0.06}px`);
  const outline = document.createElement('div');
  outline.className = 'wall-cursor';
  outline.style.borderWidth = `${CURSOR_WIDTH}px`;
  let columns = 0;
  let tiles = 0;
  let frame = 0;
  const show = () => {
    line.textContent = clock(frame);
    const at = cursor(frame, tiles);
    const x = GAP + (at % columns) * (width + GAP);
    const y = GAP + Math.trunc(at / columns) * (height + GAP);
    outline.style.transform = `translate(${x}px, ${y}px)`;
  };
  // Builds the tiles that fit in the wall's area, again only when that
  // number changes.
  const lay = () => {
    const across = fit(area.clientWidth, width);
    const count = across * fit(area.clientHeight, height);
    if (across === columns && count === tiles) {
      return;
    }
    columns = across;
    tiles = count;
    style.setProperty('--columns', String(columns));
    const grid = document.createDocumentFragment();
    for (let index = 0; index < tiles; index++) {
      const tile = element('div', 'wall-tile', grid);
      tile.style.background = PALETTE[index % PALETTE.length];
      element('div', 'wall-label', tile).textContent = label(index);
      const row = element('div', 'wall-row', tile);
      element('div', 'wall-dot', row).style.background =
        CHIP_BACKGROUND[Math.trunc(index / PALETTE.length) % CHIP_BACKGROUND.length];
      element('div', 'wall-value', row).textContent = value(index);
    }
    area.replaceChildren(grid, outline);
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
