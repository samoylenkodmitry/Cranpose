// Deterministic benchmark data: `shared-kotlin/dev/perfcompare/shared/PerfData.kt`
// and `cranpose-app/src/data.rs` implement the same generator bit for bit, so
// every app draws identical content. Only what the gauntlet shows is kept.

export const POST_COUNT = 5000;
export const BAR_COUNT = 24;
export const CARD_ROWS_PER_CLUSTER = 5;
export const AVATAR_COUNT = 8;
export const AVATAR_SIZE = 64;
export const SPARK_POINTS = 48;

const WORDS = [
  'lorem', 'ipsum', 'dolor', 'sit', 'amet', 'consectetur', 'adipiscing', 'elit', 'sed', 'do',
  'eiusmod', 'tempor', 'incididunt', 'ut', 'labore', 'et', 'dolore', 'magna', 'aliqua', 'enim',
  'ad', 'minim', 'veniam', 'quis', 'nostrud', 'exercitation', 'ullamco', 'laboris', 'nisi',
  'aliquip', 'ex', 'ea', 'commodo', 'consequat', 'duis', 'aute', 'irure', 'in', 'reprehenderit',
  'voluptate', 'velit', 'esse', 'cillum', 'fugiat', 'nulla', 'pariatur',
];
const FIRST = [
  'Ada', 'Linus', 'Grace', 'Alan', 'Barbara', 'Dennis', 'Ken', 'Margaret', 'Edsger', 'Donald',
  'Frances', 'John', 'Radia', 'Tim', 'Guido', 'Bjarne',
];
const LAST = [
  'Lovelace', 'Torvalds', 'Hopper', 'Turing', 'Liskov', 'Ritchie', 'Thompson', 'Hamilton',
  'Dijkstra', 'Knuth', 'Allen', 'McCarthy', 'Perlman', 'Berners', 'Rossum', 'Stroustrup',
];
const TAGS = [
  '#rust', '#kotlin', '#compose', '#android', '#gpu', '#layout', '#text', '#perf', '#wgpu',
  '#skia', '#ui', '#mobile',
];

export const PALETTE = [
  '#EF4444', '#F97316', '#EAB308', '#22C55E', '#14B8A6', '#3B82F6', '#8B5CF6', '#EC4899',
];
export const CHIP_BACKGROUND = [
  '#FEE2E2', '#FFEDD5', '#FEF9C3', '#DCFCE7', '#CCFBF1', '#DBEAFE', '#EDE9FE', '#FCE7F3',
];
export const GRADIENT_END = [
  '#7F1D1D', '#7C2D12', '#713F12', '#14532D', '#134E4A', '#1E3A8A', '#4C1D95', '#831843',
];

/** 32-bit xorshift, in unsigned 32-bit arithmetic as in the other apps. */
export class Rng {
  private state: number;

  constructor(seed: number) {
    this.state = (Math.imul(seed, 0x9e3779b9) ^ 0xa5a5a5a5) >>> 0;
    if (this.state === 0) {
      this.state = 1;
    }
  }

  next(): number {
    let x = this.state;
    x = (x ^ (x << 13)) >>> 0;
    x = (x ^ (x >>> 17)) >>> 0;
    x = (x ^ (x << 5)) >>> 0;
    this.state = x;
    return x;
  }

  below(bound: number): number {
    return this.next() % bound;
  }

  unit(): number {
    return (this.next() >>> 8) / 16777216;
  }
}

export interface Post {
  id: number;
  author: string;
  color: number;
  title: string;
  body: string;
  tags: [string, number][];
  stats: string[];
}

function sentence(rng: Rng, min: number, max: number): string {
  const count = min + rng.below(max - min + 1);
  const words: string[] = [];
  for (let index = 0; index < count; index++) {
    words.push(WORDS[rng.below(46)]);
  }
  const text = words.join(' ');
  return text[0].toUpperCase() + text.slice(1);
}

export function compactCount(value: number): string {
  return value >= 1000 ? `${Math.trunc(value / 1000)}.${Math.trunc((value % 1000) / 100)}k` : `${value}`;
}

export function post(index: number): Post {
  const rng = new Rng(index + 1);
  const first = FIRST[rng.below(16)];
  const last = LAST[rng.below(16)];
  rng.below(59); // minutes, in the handle the gauntlet does not show
  rng.below(1000); // the handle's number
  const color = rng.below(8);
  const title = sentence(rng, 4, 8);
  const body = `${sentence(rng, 22, 38)}.`;
  for (let bar = 0; bar < BAR_COUNT; bar++) {
    rng.unit();
  }
  const tags: [string, number][] = [];
  for (let index = 0; index < 3; index++) {
    const tag = rng.below(TAGS.length);
    tags.push([TAGS[tag], tag % 8]);
  }
  const stats: string[] = [];
  for (let index = 0; index < 4; index++) {
    stats.push(compactCount(rng.below(20000)));
  }
  return { id: index, author: `${first} ${last}`, color, title, body, tags, stats };
}

export function posts(): Post[] {
  return Array.from({ length: POST_COUNT }, (_, index) => post(index));
}

export interface GauntletTier {
  columns: number;
  scale: number;
  tickers: number;
  depth: number;
}

const TIERS: GauntletTier[] = [
  { columns: 1, scale: 1.0, tickers: 8, depth: 6 },
  { columns: 2, scale: 0.85, tickers: 12, depth: 8 },
  { columns: 2, scale: 0.7, tickers: 16, depth: 10 },
  { columns: 3, scale: 0.6, tickers: 20, depth: 12 },
  { columns: 3, scale: 0.5, tickers: 28, depth: 14 },
  { columns: 4, scale: 0.45, tickers: 36, depth: 16 },
  { columns: 4, scale: 0.4, tickers: 44, depth: 20 },
  { columns: 5, scale: 0.35, tickers: 56, depth: 24 },
];

/** Tier `1..8`; anything else is clamped into that range. */
export function gauntletTier(tier: number): GauntletTier {
  return TIERS[Math.min(Math.max(tier, 1), TIERS.length) - 1];
}

/** A ticker symbol and how its price moves. */
export interface Ticker {
  symbol: string;
  baseCents: number;
  swingCents: number;
  step: number;
  phase: number;
}

export function tickers(count: number): Ticker[] {
  const rng = new Rng(31337);
  return Array.from({ length: count }, () => {
    const length = 3 + rng.below(2);
    let symbol = '';
    for (let index = 0; index < length; index++) {
      symbol += String.fromCharCode(65 + rng.below(26));
    }
    const baseCents = 1000 + rng.below(99000);
    const swingCents = 50 + rng.below(950);
    const step = 1 + rng.below(9);
    const phase = rng.below(2000);
    return { symbol, baseCents, swingCents, step, phase };
  });
}

/** A triangle wave over `period`: 0 at the ends, `period / 2` in the middle. */
function triangle(value: number, period: number): number {
  const position = ((value % period) + period) % period;
  return period / 2 - Math.abs(position - period / 2);
}

/** The quote's price on `frame`, in cents. */
export function tickerCents(ticker: Ticker, frame: number): number {
  const wave = triangle(frame * ticker.step + ticker.phase, 2000);
  return ticker.baseCents + Math.trunc((ticker.swingCents * (wave - 500)) / 500);
}

function twoDecimals(hundredths: number): string {
  const value = Math.abs(hundredths);
  const fraction = value % 100;
  return `${Math.trunc(value / 100)}.${fraction < 10 ? '0' : ''}${fraction}`;
}

/** `1234` → `12.34`. */
export function centsText(cents: number): string {
  return `${cents < 0 ? '-' : ''}${twoDecimals(cents)}`;
}

/** The change from the base price as a signed percent: `+1.25%`. */
export function changeText(ticker: Ticker, cents: number): string {
  const basisPoints = Math.trunc(((cents - ticker.baseCents) * 10000) / ticker.baseCents);
  return `${basisPoints < 0 ? '-' : '+'}${twoDecimals(basisPoints)}%`;
}

/** A card's progress on `frame`, in thousandths. */
export function progressPermille(card: number, frame: number): number {
  return (frame * 3 + card * 37) % 1000;
}

/** The tilt of a card's badge on `frame`, in degrees: -5 to 5. */
export function badgeDegrees(card: number, frame: number): number {
  return triangle(frame * 2 + card * 30, 40) * 0.5 - 5;
}

/** The content's share of the screen width on `frame`: 0.92 to 1. */
export function widthFraction(frame: number): number {
  return 0.92 + (0.08 * triangle(frame * 3, 200)) / 100;
}

/** The sparkline's height at `point`, as a fraction of the chart, on `frame`. */
export function sparkValue(card: number, point: number, frame: number): number {
  const phase = (frame + card * 7) * 0.11;
  return 0.5 + 0.38 * Math.sin(point * 0.32 + phase) + 0.08 * Math.sin(point * 1.7 + card);
}

const AVATAR_COLORS = [
  [0xef, 0x44, 0x44, 0x7f, 0x1d, 0x1d],
  [0xf9, 0x73, 0x16, 0x7c, 0x2d, 0x12],
  [0xea, 0xb3, 0x08, 0x71, 0x3f, 0x12],
  [0x22, 0xc5, 0x5e, 0x14, 0x53, 0x2d],
  [0x14, 0xb8, 0xa6, 0x13, 0x4e, 0x4a],
  [0x3b, 0x82, 0xf6, 0x1e, 0x3a, 0x8a],
  [0x8b, 0x5c, 0xf6, 0x4c, 0x1d, 0x95],
  [0xec, 0x48, 0x99, 0x83, 0x18, 0x43],
];

/** Avatar `index` as RGBA bytes: a radial gradient crossed by diagonal stripes. */
export function avatarRgba(index: number): Uint8Array {
  const colors = AVATAR_COLORS[index % AVATAR_COUNT];
  const size = AVATAR_SIZE;
  const pixels = new Uint8Array(size * size * 4);
  let at = 0;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = x - size / 2;
      const dy = y - (size * 3) / 8;
      const t = Math.min(255, Math.trunc(((dx * dx + dy * dy) * 255) / (40 * 40)));
      const stripe = Math.trunc((x + y + index * 3) / 6) % 2 === 0;
      for (let channel = 0; channel < 3; channel++) {
        let value = Math.trunc((colors[channel] * (255 - t) + colors[channel + 3] * t) / 255);
        if (stripe) {
          value += Math.trunc((255 - value) / 6);
        }
        pixels[at++] = value;
      }
      pixels[at++] = 0xff;
    }
  }
  return pixels;
}
