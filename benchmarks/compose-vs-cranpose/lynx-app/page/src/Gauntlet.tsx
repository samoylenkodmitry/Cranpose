// The gauntlet in ReactLynx: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
// Lynx's own idiom. The benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. One frame loop on the background thread advances it, so
// each index is one React commit: small memoized components read it alone and
// re-render, and nothing else does. The loop starts a frame only once the main
// thread has applied the last one, so each index is one frame on screen. The
// list is a `<list>` of deferred items: an item's components exist only while
// it is on screen.

import {
  memo,
  useCallback,
  useEffect,
  useInitData,
  useMemo,
  useRef,
  useSyncExternalStore,
  runOnMainThread,
} from '@lynx-js/react';
import type { ReactNode } from '@lynx-js/react';
import type { CSSProperties, NodesRef } from '@lynx-js/types';

import {
  AVATAR_COUNT,
  AVATAR_SIZE,
  CARD_ROWS_PER_CLUSTER,
  CHIP_BACKGROUND,
  GRADIENT_END,
  type GauntletTier,
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
} from '../../../shared-ts/data';
import { pngDataUri } from '../../../shared-ts/png';

declare module '@lynx-js/react' {
  interface InitData {
    tier?: number;
    freeze?: number;
  }
}

const INK = '#111827';
const BODY = '#374151';
const MUTED = '#6B7280';
const HAIRLINE = '#E5E7EB';
const PANEL = '#E2E8F0';
const UP = '#16A34A';
const DOWN = '#DC2626';
const WHITE = '#FFFFFF';
const LEVEL_BACKGROUND = ['#F1F5F9', '#CBD5E1'];
const FOOTER_LABELS = ['likes', 'replies', 'shares'];

/**
 * Blocks of five card rows and a cluster. Every row is a `<list-item>` the
 * list keeps, so the list holds only what a measurement can reach: a window
 * scrolls 3 dp × 120 Hz × 20 s = 7,200 dp at most, and 200 blocks of the
 * smallest tier are over 60,000 dp.
 */
const BLOCKS = 200;
const ROWS_PER_BLOCK = CARD_ROWS_PER_CLUSTER + 1;

/** How far the list scrolls each frame, in dp. */
const SCROLL_PER_FRAME = 3;

/**
 * About how tall a row of cards and a cluster level are at scale 1, in dp,
 * from the parts they stack: the list places a row it has not rendered yet
 * at this height, in device pixels, and asks for as many rows as the
 * estimates fill.
 */
const CARD_ROW_ESTIMATE = 262;
const LEVEL_ESTIMATE = 13.5;

/** The frame index every per-frame value follows. */
class FrameClock {
  private frame = 0;
  private readonly listeners = new Set<() => void>();

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  current = () => this.frame;

  advance(): number {
    this.frame += 1;
    this.listeners.forEach((listener) => listener());
    return this.frame;
  }
}

const clock = new FrameClock();

/** The frame index, re-rendering the caller alone when it advances. */
function useFrame(): number {
  return useSyncExternalStore(clock.subscribe, clock.current);
}

/** A length in dp. Lynx does not read a number in a style object as a CSS
 * number, so every value here is a string: lengths in `px`, `flex` and
 * `opacity` bare. */
function px(value: number): string {
  return `${value}px`;
}

/** Text of `size` in `color`, in the device's Roboto, which the app registers as `PerfRoboto`. */
function text(size: number, color: string, bold = false): CSSProperties {
  return { fontFamily: 'PerfRoboto', fontSize: px(size), color, fontWeight: bold ? 'bold' : 'normal' };
}

/** Paragraph text: lines 1.4 em apart, cut with an ellipsis. */
function paragraph(size: number, color: string, bold = false): CSSProperties {
  return { ...text(size, color, bold), lineHeight: px(size * 1.4), overflow: 'hidden', textOverflow: 'ellipsis' };
}

/** What one tier's screen needs to place and size its elements. */
interface Layout {
  s: number;
  columns: number;
  styles: ReturnType<typeof createStyles>;
}

function createStyles(s: number) {
  const row: CSSProperties = { display: 'flex', flexDirection: 'row' };
  const column: CSSProperties = { display: 'flex', flexDirection: 'column' };
  return {
    panel: { ...row, flexWrap: 'wrap', gap: px(4 * s), padding: px(6 * s), backgroundColor: PANEL },
    tile: {
      ...row,
      alignItems: 'center',
      gap: px(4 * s),
      backgroundColor: WHITE,
      borderRadius: px(6 * s),
      padding: `${px(3 * s)} ${px(6 * s)}`,
    },
    symbol: text(10 * s, INK, true),
    price: text(10 * s, BODY),
    up: text(10 * s, UP),
    down: text(10 * s, DOWN),
    tileBar: { width: px(20 * s), height: px(4 * s) },
    list: { flex: '1', padding: px(8 * s), listMainAxisGap: px(8 * s) },
    // Lynx lays a list row's children out linearly without stretching them
    // to the row's width, so the rows take it themselves.
    cardRow: { ...row, width: '100%', alignItems: 'flex-start', gap: px(8 * s) },
    cell: { ...column, flex: '1' },
    cardBox: column,
    card: {
      ...column,
      backgroundColor: WHITE,
      borderRadius: px(12 * s),
      border: `1px solid ${HAIRLINE}`,
      // Compose draws its border over the padding; Lynx insets the content
      // by both, so the padding gives the border's width back.
      padding: px(10 * s - 1),
      gap: px(6 * s),
      boxShadow: `0 ${px(s)} ${px(3 * s)} rgba(0, 0, 0, 0.24)`,
    },
    header: { ...row, alignItems: 'center', gap: px(8 * s) },
    avatar: { width: px(32 * s), height: px(32 * s), borderRadius: px(16 * s) },
    headerText: { ...column, flex: '1' },
    title: paragraph(13 * s, INK, true),
    subtitle: { ...text(11 * s, MUTED), overflow: 'hidden', textOverflow: 'ellipsis' },
    // A nested text inherits no style from the text around it.
    author: text(11 * s, MUTED, true),
    body: paragraph(12 * s, BODY),
    progressRow: { ...row, alignItems: 'center', gap: px(6 * s) },
    progressBar: { flex: '1', height: px(6 * s) },
    percent: text(10 * s, MUTED),
    sparkline: { height: px(36 * s) },
    chips: { ...row, flexWrap: 'wrap', gap: px(4 * s) },
    chip: { borderRadius: px(10 * s), padding: `${px(3 * s)} ${px(8 * s)}` },
    chipText: text(10 * s, INK),
    footer: row,
    divider: { width: '1px', backgroundColor: HAIRLINE },
    counter: { ...column, flex: '1', alignItems: 'center' },
    stat: text(12 * s, INK, true),
    statLabel: text(9 * s, MUTED),
    badge: {
      position: 'absolute',
      top: px(6 * s),
      right: px(6 * s),
      opacity: '0.9',
      backgroundColor: PALETTE[0],
      borderRadius: px(8 * s),
      padding: `${px(2 * s)} ${px(6 * s)}`,
    },
    badgeText: text(9 * s, WHITE, true),
    level: {
      ...column,
      width: '100%',
      borderRadius: px(4 * s),
      padding: `${px(s)} ${px(s)} ${px(s)} ${px(3 * s)}`,
      gap: px(s),
    },
    levelRow: { ...row, alignItems: 'flex-start', gap: px(2 * s) },
    levelChip: { flex: '1', borderRadius: px(2 * s) },
    levelText: text(9 * s, WHITE),
  } satisfies Record<string, CSSProperties>;
}

export function Gauntlet() {
  const { tier = 5, freeze = 0 } = useInitData();
  const load = gauntletTier(tier);
  const layout = useMemo<Layout>(
    () => ({ s: load.scale, columns: load.columns, styles: createStyles(load.scale) }),
    [load],
  );
  const content = useMemo(
    () => ({
      posts: posts(),
      avatars: Array.from({ length: AVATAR_COUNT }, (_, index) =>
        pngDataUri(avatarRgba(index), AVATAR_SIZE, AVATAR_SIZE),
      ),
      quotes: tickers(load.tickers),
    }),
    [load],
  );
  return (
    // The page gives its child no size to take a share of, so the screen
    // pins itself to the page's edges.
    <view
      style={{
        display: 'flex',
        flexDirection: 'column',
        position: 'absolute',
        top: '0px',
        left: '0px',
        right: '0px',
        bottom: '0px',
        backgroundColor: '#EEF0F5',
      }}
    >
      <view
        style={{
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'center',
          height: '56px',
          padding: '0 16px',
          backgroundColor: '#1E2A4A',
        }}
      >
        <text style={text(20, WHITE, true)}>Gauntlet</text>
      </view>
      <view style={{ display: 'flex', flexDirection: 'column', flex: '1' }}>
        <WidthFollower>
          <TickerPanel quotes={content.quotes} layout={layout} />
          <CardList tier={load} posts={content.posts} avatars={content.avatars} freeze={freeze} layout={layout} />
        </WidthFollower>
      </view>
    </view>
  );
}

/** The content's width in whole pixels on `frame`, in dp. */
function contentWidth(frame: number): number {
  return Math.round(SystemInfo.pixelWidth * widthFraction(frame)) / SystemInfo.pixelRatio;
}

/**
 * Lays its children out at the frame's share of the width. The children are
 * elements its parent made once, so the frame re-renders this view alone and
 * Lynx lays everything out again.
 */
function WidthFollower({ children }: { children: ReactNode }) {
  const frame = useFrame();
  return <view style={{ display: 'flex', flexDirection: 'column', flex: '1', width: px(contentWidth(frame)) }}>{children}</view>;
}

/** A rounded track filled to `share` of the frame in `color`. */
const Bar = memo(function Bar({
  share,
  color,
  radius,
  style,
}: {
  share: (frame: number) => number;
  color: string;
  radius: number;
  style: CSSProperties;
}) {
  const frame = useFrame();
  return (
    <view style={{ ...style, backgroundColor: HAIRLINE, borderRadius: px(radius) }}>
      <view style={{ width: `${share(frame) * 100}%`, height: '100%', backgroundColor: color, borderRadius: px(radius) }} />
    </view>
  );
});

const TickerPanel = memo(function TickerPanel({ quotes, layout }: { quotes: Ticker[]; layout: Layout }) {
  return (
    <view style={layout.styles.panel}>
      {quotes.map((quote, index) => (
        <TickerTile key={quote.symbol + index} quote={quote} index={index} layout={layout} />
      ))}
    </view>
  );
});

const TickerTile = memo(function TickerTile({ quote, index, layout }: { quote: Ticker; index: number; layout: Layout }) {
  const { styles, s } = layout;
  const share = useCallback(
    (frame: number) =>
      Math.min(1, Math.max(0, (tickerCents(quote, frame) - quote.baseCents + quote.swingCents) / (2 * quote.swingCents))),
    [quote],
  );
  return (
    <view style={styles.tile}>
      <text style={styles.symbol}>{quote.symbol}</text>
      <TickerPrice quote={quote} layout={layout} />
      <TickerChange quote={quote} layout={layout} />
      <Bar share={share} color={PALETTE[index % PALETTE.length]} radius={2 * s} style={styles.tileBar} />
    </view>
  );
});

/** The price on this frame: its own component, so the frame re-renders only it. */
function TickerPrice({ quote, layout }: { quote: Ticker; layout: Layout }) {
  const frame = useFrame();
  return <text style={layout.styles.price}>{centsText(tickerCents(quote, frame))}</text>;
}

/** The signed change, green or red, in its own component like the price. */
function TickerChange({ quote, layout }: { quote: Ticker; layout: Layout }) {
  const frame = useFrame();
  const cents = tickerCents(quote, frame);
  return <text style={cents >= quote.baseCents ? layout.styles.up : layout.styles.down}>{changeText(quote, cents)}</text>;
}

const CardList = memo(function CardList({
  tier,
  posts: all,
  avatars,
  freeze,
  layout,
}: {
  tier: GauntletTier;
  posts: Post[];
  avatars: string[];
  freeze: number;
  layout: Layout;
}) {
  const list = useRef<NodesRef>(null);

  const rows = [];
  for (let row = 0; row < BLOCKS * ROWS_PER_BLOCK; row++) {
    const block = Math.trunc(row / ROWS_PER_BLOCK);
    const within = row % ROWS_PER_BLOCK;
    const key = `row-${row}`;
    if (within === CARD_ROWS_PER_CLUSTER) {
      rows.push(
        <list-item
          key={key}
          item-key={key}
          reuse-identifier='cluster'
          estimated-main-axis-size-px={(tier.depth + 1) * LEVEL_ESTIMATE * layout.s * SystemInfo.pixelRatio}
          defer={{ unmountRecycled: true }}
        >
          <Level cluster={block} remaining={tier.depth} layout={layout} />
        </list-item>,
      );
    } else {
      const first = (block * CARD_ROWS_PER_CLUSTER + within) * tier.columns;
      rows.push(
        <list-item
          key={key}
          item-key={key}
          reuse-identifier='cards'
          estimated-main-axis-size-px={CARD_ROW_ESTIMATE * layout.s * SystemInfo.pixelRatio}
          defer={{ unmountRecycled: true }}
        >
          <CardRow first={first} posts={all} avatars={avatars} layout={layout} />
        </list-item>,
      );
    }
  }

  return (
    <>
      <list
        ref={list}
        custom-list-name='list-container'
        list-type='single'
        span-count={1}
        scroll-orientation='vertical'
        style={layout.styles.list}
      >
        {rows}
      </list>
      <FrameLoop list={list} freeze={freeze} />
    </>
  );
});

/**
 * Advances the frame and scrolls the list once the main thread has applied
 * the last frame. Effects run after the frame's commit is sent, and the main
 * thread runs `applied` after every update sent before it, so the background
 * thread never runs ahead of the screen.
 */
function FrameLoop({ list, freeze }: { list: { current: NodesRef | null }; freeze: number }) {
  const frame = useFrame();
  const applied = () => {
    'main thread';
  };
  useEffect(() => {
    let request = 0;
    void runOnMainThread(applied)().then(() => {
      if (freeze > 0 && frame >= freeze) {
        NativeModules.PerfLog.line(`PERF frozen frame=${frame}`);
        return;
      }
      request = requestAnimationFrame(() => {
        clock.advance();
        list.current?.invoke({ method: 'scrollBy', params: { offset: SCROLL_PER_FRAME } }).exec();
      });
    });
    return () => cancelAnimationFrame(request);
  }, [frame]);
  return null;
}

const CardRow = memo(function CardRow({
  first,
  posts: all,
  avatars,
  layout,
}: {
  first: number;
  posts: Post[];
  avatars: string[];
  layout: Layout;
}) {
  const cells = [];
  for (let column = 0; column < layout.columns; column++) {
    const card = first + column;
    cells.push(
      <view key={column} style={layout.styles.cell}>
        <Card post={all[card % POST_COUNT]} avatar={avatars[card % AVATAR_COUNT]} card={card} layout={layout} />
      </view>,
    );
  }
  return <view style={layout.styles.cardRow}>{cells}</view>;
});

const Card = memo(function Card({ post, avatar, card, layout }: { post: Post; avatar: string; card: number; layout: Layout }) {
  const { styles, s } = layout;
  const [tag, tagColor] = post.tags[0];
  const share = useCallback((frame: number) => progressPermille(card, frame) / 1000, [card]);
  return (
    <view style={styles.cardBox}>
      <view style={styles.card}>
        <view style={styles.header}>
          <image src={avatar} mode='aspectFill' style={styles.avatar} />
          <view style={styles.headerText}>
            <text style={styles.title} text-maxline='2'>
              {post.title}
            </text>
            {/* `by Ada Lovelace · #rust`: the author bold, the tag in its color. */}
            <text style={styles.subtitle} text-maxline='1'>
              by <text style={styles.author}>{post.author}</text>
              {' · '}
              <text style={{ ...styles.subtitle, color: GRADIENT_END[tagColor] }}>{tag}</text>
            </text>
          </view>
        </view>
        <text style={styles.body} text-maxline='4'>
          {post.body}
        </text>
        <view style={styles.progressRow}>
          <Bar share={share} color={PALETTE[card % PALETTE.length]} radius={3 * s} style={styles.progressBar} />
          <ProgressLabel card={card} layout={layout} />
        </view>
        <Sparkline card={card} color={PALETTE[post.color]} layout={layout} />
        <view style={styles.chips}>
          {post.tags.map(([chip, color], index) => (
            <view key={index} style={{ ...styles.chip, backgroundColor: CHIP_BACKGROUND[color] }}>
              <text style={{ ...styles.chipText, color: GRADIENT_END[color] }}>{chip}</text>
            </view>
          ))}
        </view>
        <Footer stats={post.stats} layout={layout} />
      </view>
      {card % 5 === 0 ? <Badge card={card} layout={layout} /> : null}
    </view>
  );
});

/** The percent beside the bar, in its own component like the ticker's labels. */
function ProgressLabel({ card, layout }: { card: number; layout: Layout }) {
  const frame = useFrame();
  return <text style={layout.styles.percent}>{`${Math.trunc(progressPermille(card, frame) / 10)}%`}</text>;
}

/**
 * A card's 48-point line over a fading fill, drawn from the frame as SVG. The
 * chart is as wide as a card's content on this frame, which follows from the
 * frame's width like the rest of the layout.
 */
function Sparkline({ card, color, layout }: { card: number; color: string; layout: Layout }) {
  const frame = useFrame();
  const { s, columns, styles } = layout;
  const inner = contentWidth(frame) - 16 * s;
  const width = (inner - (columns - 1) * 8 * s) / columns - 20 * s;
  const height = 36 * s;
  const step = width / (SPARK_POINTS - 1);
  let line = '';
  for (let index = 0; index < SPARK_POINTS; index++) {
    const y = height * (1 - sparkValue(card, index, frame));
    line += `${index === 0 ? 'M' : 'L'}${(index * step).toFixed(2)} ${y.toFixed(2)}`;
  }
  const content =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">`
    + `<defs><linearGradient id="f" x1="0" y1="0" x2="0" y2="${height}" gradientUnits="userSpaceOnUse">`
    + `<stop offset="0" stop-color="${color}" stop-opacity="0.25"/><stop offset="1" stop-color="${color}" stop-opacity="0"/>`
    + `</linearGradient></defs>`
    + `<path d="M0 ${height}L${line.slice(1)}L${width} ${height}Z" fill="url(#f)"/>`
    + `<path d="${line}" fill="none" stroke="${color}" stroke-width="${1.5 * s}"/></svg>`;
  return <svg content={content} style={{ ...styles.sparkline, width: px(width) }} />;
}

/** Three counters split by dividers as tall as the row: the flex row stretches them. */
const Footer = memo(function Footer({ stats, layout }: { stats: string[]; layout: Layout }) {
  const { styles } = layout;
  const cells = [];
  for (let index = 0; index < FOOTER_LABELS.length; index++) {
    if (index > 0) {
      cells.push(<view key={`divider-${index}`} style={styles.divider} />);
    }
    cells.push(
      <view key={FOOTER_LABELS[index]} style={styles.counter}>
        <text style={styles.stat}>{stats[index]}</text>
        <text style={styles.statLabel}>{FOOTER_LABELS[index]}</text>
      </view>,
    );
  }
  return <view style={styles.footer}>{cells}</view>;
});

/** A translucent tag tilting with the frame. */
function Badge({ card, layout }: { card: number; layout: Layout }) {
  const frame = useFrame();
  return (
    <view style={{ ...layout.styles.badge, transform: `rotate(${badgeDegrees(card, frame)}deg)` }}>
      <text style={layout.styles.badgeText}>HOT</text>
    </view>
  );
}

/**
 * `depth` levels nested inside one another, each a row of three labels above
 * the next level.
 */
const Level = memo(function Level({ cluster, remaining, layout }: { cluster: number; remaining: number; layout: Layout }) {
  const { styles } = layout;
  return (
    <view style={{ ...styles.level, backgroundColor: LEVEL_BACKGROUND[remaining % 2] }}>
      <view style={styles.levelRow}>
        {[0, 1, 2].map((chip) => (
          <view
            key={chip}
            style={{ ...styles.levelChip, backgroundColor: PALETTE[(remaining + chip + cluster) % PALETTE.length] }}
          >
            <text style={styles.levelText}>{`C${cluster}.L${remaining}.${chip}`}</text>
          </view>
        ))}
      </view>
      {remaining > 0 ? <Level cluster={cluster} remaining={remaining - 1} layout={layout} /> : null}
    </view>
  );
});
