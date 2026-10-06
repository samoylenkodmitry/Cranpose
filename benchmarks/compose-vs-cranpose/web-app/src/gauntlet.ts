// The gauntlet as a web page: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in the web
// platform's idiom. CSS (`www/style.css`) lays everything out; the DOM holds
// only the rows on screen, recycled as they scroll off; one animation frame
// loop advances the frame and sets only what changed. The benchmark's README
// describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time.

import {
  AVATAR_COUNT,
  AVATAR_SIZE,
  CARD_ROWS_PER_CLUSTER,
  CHIP_BACKGROUND,
  GRADIENT_END,
  PALETTE,
  POST_COUNT,
  type Post,
  SPARK_POINTS,
  type Ticker,
  avatarRgba,
  badgeDegrees,
  centsText,
  changeText,
  gauntletTier,
  posts,
  progressPermille,
  sparkValue,
  tickerCents,
  tickers,
  widthFraction,
} from '../../shared-ts/data.js';

interface Launch {
  tier: number;
  freeze: number;
}

/** The Android app's plugin: what `am start` asked, and `PERF` lines under the other apps' log tag. */
interface LaunchPlugin {
  get(): Promise<Launch>;
  log(options: { message: string }): Promise<void>;
}

declare global {
  interface Window {
    Capacitor?: { Plugins: { Launch?: LaunchPlugin } };
    /** The Tauri app's commands: the same two. */
    __TAURI__?: { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };
  }
}

/** What the app around the page asked, and where its `PERF` lines go: the
 * Android app's plugin, the Tauri app's commands, or in a desktop browser the
 * page's address and the console `desktop.py` reads. */
function host(): { launch: Promise<Launch>; log(message: string): void } {
  const plugin = window.Capacitor?.Plugins.Launch;
  if (plugin) {
    return { launch: plugin.get(), log: (message) => void plugin.log({ message }) };
  }
  const tauri = window.__TAURI__?.core;
  if (tauri) {
    return { launch: tauri.invoke<Launch>('launch'), log: (message) => void tauri.invoke('log', { message }) };
  }
  const query = new URLSearchParams(location.search);
  return {
    launch: Promise.resolve({ tier: Number(query.get('tier') ?? 5), freeze: Number(query.get('freeze') ?? 0) }),
    log: (message) => console.log(message),
  };
}

/** Blocks of five card rows and a cluster: no measurement window reaches the end. */
const ROWS = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/** How far the list scrolls each frame, in CSS pixels (dp). */
const SCROLL_PER_FRAME = 3;
const FOOTER_LABELS = ['likes', 'replies', 'shares'];
const LEVEL_BACKGROUND = ['#F1F5F9', '#CBD5E1'];
const SVG = 'http://www.w3.org/2000/svg';

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, parent?: Element) {
  const created = document.createElement(tag);
  created.className = className;
  parent?.append(created);
  return created;
}

function svg<K extends keyof SVGElementTagNameMap>(tag: K, attributes: Record<string, string>, parent?: Element) {
  const created = document.createElementNS(SVG, tag);
  for (const [name, value] of Object.entries(attributes)) {
    created.setAttribute(name, value);
  }
  parent?.append(created);
  return created;
}

/** The page's element for `selector`, which `index.html` declares. */
function need<T extends Element>(selector: string): T {
  const found = document.querySelector<T>(selector);
  if (!found) {
    throw new Error(`index.html has no ${selector}`);
  }
  return found;
}

/** Sets the text only when it changed, so an unchanged frame touches nothing. */
function setText(target: Element, text: string) {
  if (target.textContent !== text) {
    target.textContent = text;
  }
}

/** A rounded track filled to a share of it. */
function bar(parent: Element) {
  const track = element('div', 'track', parent);
  return element('div', 'fill', track);
}

class TickerTile {
  private readonly price: HTMLSpanElement;
  private readonly change: HTMLSpanElement;
  private readonly fill: HTMLDivElement;

  constructor(private readonly ticker: Ticker, index: number, panel: Element) {
    const tile = element('div', 'tile', panel);
    element('span', 'symbol', tile).textContent = ticker.symbol;
    this.price = element('span', '', tile);
    this.change = element('span', '', tile);
    this.fill = bar(tile);
    this.fill.style.background = PALETTE[index % PALETTE.length];
  }

  update(frame: number) {
    const { ticker } = this;
    const cents = tickerCents(ticker, frame);
    setText(this.price, centsText(cents));
    setText(this.change, changeText(ticker, cents));
    this.change.className = cents >= ticker.baseCents ? 'up' : 'down';
    const share = Math.min(1, Math.max(0, (cents - ticker.baseCents + ticker.swingCents) / (2 * ticker.swingCents)));
    this.fill.style.width = `${share * 100}%`;
  }
}

/** A card, built once and bound to whichever card its row shows. */
class Card {
  readonly element: HTMLDivElement;
  private readonly avatar: HTMLImageElement;
  private readonly title: HTMLDivElement;
  private readonly author: HTMLElement;
  private readonly tag: HTMLSpanElement;
  private readonly body: HTMLDivElement;
  private readonly fill: HTMLDivElement;
  private readonly percent: HTMLSpanElement;
  private readonly area: SVGPathElement;
  private readonly line: SVGPathElement;
  private readonly chips: HTMLSpanElement[] = [];
  private readonly stats: HTMLSpanElement[] = [];
  private readonly badge: HTMLDivElement;
  private card = 0;

  constructor(parent: Element) {
    this.element = element('div', 'card', parent);
    const header = element('div', 'header', this.element);
    this.avatar = element('img', 'avatar', header);
    const headText = element('div', 'head-text', header);
    this.title = element('div', 'paragraph title', headText);
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    const subtitle = element('div', 'subtitle', headText);
    subtitle.append('by ');
    this.author = element('b', '', subtitle);
    subtitle.append(' · ');
    this.tag = element('span', '', subtitle);
    this.body = element('div', 'paragraph body', this.element);
    const progress = element('div', 'progress', this.element);
    this.fill = bar(progress);
    this.percent = element('span', 'percent', progress);
    const sparkline = svg('svg', { class: 'sparkline', viewBox: `0 0 ${SPARK_POINTS - 1} 1`, preserveAspectRatio: 'none' }, this.element);
    this.area = svg('path', {}, sparkline);
    this.line = svg('path', { class: 'line' }, sparkline);
    const chips = element('div', 'chips', this.element);
    for (let index = 0; index < 3; index++) {
      this.chips.push(element('span', 'chip', chips));
    }
    const footer = element('div', 'footer', this.element);
    FOOTER_LABELS.forEach((label, index) => {
      if (index > 0) {
        element('div', 'divider', footer);
      }
      const counter = element('div', 'counter', footer);
      this.stats.push(element('span', 'stat', counter));
      element('span', 'label', counter).textContent = label;
    });
    this.badge = element('div', 'badge', this.element);
    this.badge.textContent = 'HOT';
  }

  bind(card: number, post: Post, avatar: string) {
    this.card = card;
    this.avatar.src = avatar;
    this.title.textContent = post.title;
    this.author.textContent = post.author;
    const [tag, tagColor] = post.tags[0];
    this.tag.textContent = tag;
    this.tag.style.color = GRADIENT_END[tagColor];
    this.body.textContent = post.body;
    this.fill.style.background = PALETTE[card % PALETTE.length];
    this.area.setAttribute('fill', `url(#spark-${post.color})`);
    this.line.setAttribute('stroke', PALETTE[post.color]);
    post.tags.forEach(([text, color], index) => {
      const chip = this.chips[index];
      chip.textContent = text;
      chip.style.background = CHIP_BACKGROUND[color];
      chip.style.color = GRADIENT_END[color];
    });
    this.stats.forEach((stat, index) => {
      stat.textContent = post.stats[index];
    });
    this.badge.hidden = card % 5 !== 0;
  }

  update(frame: number) {
    const { card } = this;
    const permille = progressPermille(card, frame);
    setText(this.percent, `${Math.trunc(permille / 10)}%`);
    this.fill.style.width = `${permille / 10}%`;
    // The sparkline in its 0..47 by 0..1 box, which CSS stretches to the card.
    let line = '';
    for (let index = 0; index < SPARK_POINTS; index++) {
      line += `${index === 0 ? 'M' : 'L'}${index} ${(1 - sparkValue(card, index, frame)).toFixed(4)}`;
    }
    this.line.setAttribute('d', line);
    this.area.setAttribute('d', `M0 1L${line.slice(1)}L${SPARK_POINTS - 1} 1Z`);
    if (!this.badge.hidden) {
      this.badge.style.transform = `rotate(${badgeDegrees(card, frame)}deg)`;
    }
  }
}

/** `depth` levels nested inside one another, each a row of three labels above the next level. */
class Cluster {
  private readonly levels: { remaining: number; level: HTMLDivElement; chips: HTMLDivElement[] }[] = [];

  constructor(parent: Element, depth: number) {
    let container = parent;
    for (let remaining = depth; remaining >= 0; remaining--) {
      const level = element('div', 'level', container);
      level.style.background = LEVEL_BACKGROUND[remaining % 2];
      const row = element('div', 'level-row', level);
      const chips = [0, 1, 2].map(() => element('div', 'level-chip', row));
      this.levels.push({ remaining, level, chips });
      container = level;
    }
  }

  bind(cluster: number) {
    for (const { remaining, chips } of this.levels) {
      chips.forEach((chip, index) => {
        chip.textContent = `C${cluster}.L${remaining}.${index}`;
        chip.style.background = PALETTE[(remaining + index + cluster) % PALETTE.length];
      });
    }
  }
}

/** A list row: cards side by side, or a cluster. */
interface Row {
  element: HTMLDivElement;
  cards: Card[];
  cluster?: Cluster;
}

async function main() {
  const { launch: launched, log } = host();
  const launch = await launched;
  const tier = gauntletTier(launch.tier);
  document.documentElement.style.setProperty('--s', String(tier.scale));

  const content = need<HTMLElement>('.content');
  const panel = need<HTMLElement>('.panel');
  const list = need<HTMLElement>('.list');
  const inner = need<HTMLElement>('.list-inner');
  const spacer = need<HTMLElement>('.spacer');
  const defs = need<SVGDefsElement>('.gradients defs');
  PALETTE.forEach((color, index) => {
    const gradient = svg('linearGradient', { id: `spark-${index}`, gradientUnits: 'userSpaceOnUse', x1: '0', y1: '0', x2: '0', y2: '1' }, defs);
    svg('stop', { offset: '0', 'stop-color': color, 'stop-opacity': '0.25' }, gradient);
    svg('stop', { offset: '1', 'stop-color': color, 'stop-opacity': '0' }, gradient);
  });

  const all = posts();
  const avatars = Array.from({ length: AVATAR_COUNT }, (_, index) => {
    const canvas = document.createElement('canvas');
    canvas.width = AVATAR_SIZE;
    canvas.height = AVATAR_SIZE;
    const pixels = new ImageData(Uint8ClampedArray.from(avatarRgba(index)), AVATAR_SIZE, AVATAR_SIZE);
    canvas.getContext('2d')?.putImageData(pixels, 0, 0);
    return canvas.toDataURL();
  });
  const tiles = tickers(tier.tickers).map((ticker, index) => new TickerTile(ticker, index, panel));

  // Rows on screen, the next row to show, and rows scrolled off for reuse.
  const rows: Row[] = [];
  const pool: { cards: Row[]; clusters: Row[] } = { cards: [], clusters: [] };
  let next = 0;
  let spacerHeight = 0;

  function take(row: number): Row {
    const block = Math.trunc(row / (CARD_ROWS_PER_CLUSTER + 1));
    const within = row % (CARD_ROWS_PER_CLUSTER + 1);
    if (within === CARD_ROWS_PER_CLUSTER) {
      const taken = pool.clusters.pop() ?? (() => {
        const created = element('div', 'row');
        return { element: created, cards: [], cluster: new Cluster(created, tier.depth) };
      })();
      taken.cluster?.bind(block);
      return taken;
    }
    const taken = pool.cards.pop() ?? (() => {
      const created = element('div', 'row');
      return { element: created, cards: Array.from({ length: tier.columns }, () => new Card(created)) };
    })();
    const first = (block * CARD_ROWS_PER_CLUSTER + within) * tier.columns;
    taken.cards.forEach((card, column) => {
      const index = first + column;
      card.bind(index, all[index % POST_COUNT], avatars[index % AVATAR_COUNT]);
    });
    return taken;
  }

  /** Shows the rows that reach the viewport scrolled to `top`, and hands
   * rows scrolled above it back to the pool behind a spacer as tall. */
  function fill(top: number, frame: number) {
    const bottom = top + list.clientHeight;
    while (next < ROWS) {
      const last = rows[rows.length - 1];
      if (last && last.element.offsetTop + last.element.offsetHeight >= bottom) {
        break;
      }
      const row = take(next++);
      inner.append(row.element);
      row.cards.forEach(card => card.update(frame));
      rows.push(row);
    }
    while (rows.length > 1 && rows[1].element.offsetTop <= top) {
      const gone = rows.shift();
      if (!gone) {
        break;
      }
      spacerHeight += rows[0].element.offsetTop - gone.element.offsetTop;
      spacer.style.height = `${spacerHeight}px`;
      gone.element.remove();
      (gone.cluster ? pool.clusters : pool.cards).push(gone);
    }
  }

  let frame = 0;
  const tick = () => {
    frame += 1;
    // The width follows the frame, in whole pixels.
    content.style.width = `${Math.round(innerWidth * devicePixelRatio * widthFraction(frame)) / devicePixelRatio}px`;
    tiles.forEach(tile => tile.update(frame));
    rows.forEach(row => row.cards.forEach(card => card.update(frame)));
    const top = frame * SCROLL_PER_FRAME;
    fill(top, frame);
    list.scrollTop = top;
    if (launch.freeze > 0 && frame >= launch.freeze) {
      log(`PERF frozen frame=${frame}`);
      return;
    }
    requestAnimationFrame(tick);
  };
  fill(0, 0);
  requestAnimationFrame(tick);
  if (!window.Capacitor?.Plugins.Launch) {
    // Outside the Android app nothing else reports the first frame.
    requestAnimationFrame(() => log('PERF first_frame'));
  }
}

main();
