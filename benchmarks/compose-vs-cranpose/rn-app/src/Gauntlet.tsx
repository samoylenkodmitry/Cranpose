// The gauntlet in React Native: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
// React Native's own idiom. The benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. One JavaScript frame loop advances it, so each index is one
// React commit: small memoized components read it alone and re-render, and
// nothing else does.

import React, { memo, useCallback, useEffect, useMemo, useRef, useSyncExternalStore } from 'react';
import {
  Image,
  type ImageSourcePropType,
  PixelRatio,
  StyleSheet,
  Text,
  type TextStyle,
  View,
  type ViewStyle,
  useWindowDimensions,
} from 'react-native';
import { FlashList, type FlashListRef, type ListRenderItem } from '@shopify/flash-list';
import { Canvas, LinearGradient, Path, Skia, vec } from '@shopify/react-native-skia';
import {
  AVATAR_COUNT,
  AVATAR_SIZE,
  CARD_ROWS_PER_CLUSTER,
  CHIP_BACKGROUND,
  GRADIENT_END,
  type GauntletTier,
  LAYER_COLUMNS,
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
  layerCellColor,
  layerDegrees,
  layerX,
  layerY,
  posts,
  progressPermille,
  sparkValue,
  tickerCents,
  tickers,
  widthFraction,
} from '../../shared-ts/data';
import { pngDataUri } from '../../shared-ts/png';

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

/** Blocks of five card rows and a cluster: no measurement window reaches the end. */
const BLOCKS = 2000;
const ROWS_PER_BLOCK = CARD_ROWS_PER_CLUSTER + 1;

/** How far the list scrolls each frame, in dp. */
const SCROLL_PER_FRAME = 3;

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
    this.listeners.forEach(listener => listener());
    return this.frame;
  }
}

const clock = new FrameClock();

/** The frame index, re-rendering the caller alone when it advances. */
function useFrame(): number {
  return useSyncExternalStore(clock.subscribe, clock.current);
}

/** Text of `size` in `color`, in the device's Roboto (registered natively), without the font's extra padding. */
function text(size: number, color: string, bold = false): TextStyle {
  return {
    fontFamily: 'PerfRoboto',
    fontSize: size,
    color,
    fontWeight: bold ? 'bold' : 'normal',
    includeFontPadding: false,
  };
}

/** Paragraph text: lines 1.4 em apart. */
function paragraph(size: number, color: string, bold = false): TextStyle {
  return { ...text(size, color, bold), lineHeight: size * 1.4 };
}

/** What one tier's screen needs to place and size its elements. */
interface Layout {
  s: number;
  columns: number;
  windowWidth: number;
  pixelRatio: number;
  styles: ReturnType<typeof createStyles>;
}

function createStyles(s: number) {
  return StyleSheet.create({
    panel: { backgroundColor: PANEL, padding: 6 * s, flexDirection: 'row', flexWrap: 'wrap', gap: 4 * s },
    tile: {
      flexDirection: 'row',
      alignItems: 'center',
      gap: 4 * s,
      backgroundColor: WHITE,
      borderRadius: 6 * s,
      paddingHorizontal: 6 * s,
      paddingVertical: 3 * s,
    },
    symbol: text(10 * s, INK, true),
    price: text(10 * s, BODY),
    up: text(10 * s, UP),
    down: text(10 * s, DOWN),
    tileBar: { width: 20 * s, height: 4 * s },
    list: { padding: 8 * s },
    separator: { height: 8 * s },
    cardRow: { flexDirection: 'row', alignItems: 'flex-start', gap: 8 * s },
    cell: { flex: 1 },
    card: {
      backgroundColor: WHITE,
      borderRadius: 12 * s,
      borderWidth: 1,
      borderColor: HAIRLINE,
      // Compose draws its border over the padding; Yoga insets the content
      // by both, so the padding gives the border's width back.
      padding: 10 * s - 1,
      gap: 6 * s,
      elevation: 3 * s,
    },
    header: { flexDirection: 'row', alignItems: 'center', gap: 8 * s },
    avatar: { width: 32 * s, height: 32 * s, borderRadius: 16 * s },
    headerText: { flex: 1 },
    title: paragraph(13 * s, INK, true),
    subtitle: text(11 * s, MUTED),
    bold: { fontWeight: 'bold' },
    body: paragraph(12 * s, BODY),
    progressRow: { flexDirection: 'row', alignItems: 'center', gap: 6 * s },
    progressBar: { flex: 1, height: 6 * s },
    percent: text(10 * s, MUTED),
    sparkline: { height: 36 * s },
    chips: { flexDirection: 'row', flexWrap: 'wrap', gap: 4 * s },
    chip: { borderRadius: 10 * s, paddingHorizontal: 8 * s, paddingVertical: 3 * s },
    chipText: text(10 * s, INK),
    footer: { flexDirection: 'row' },
    divider: { width: 1, backgroundColor: HAIRLINE },
    counter: { flex: 1, alignItems: 'center' },
    stat: text(12 * s, INK, true),
    statLabel: text(9 * s, MUTED),
    // Elevated as high as the card so it draws above it, without a shadow.
    badge: {
      position: 'absolute',
      top: 6 * s,
      right: 6 * s,
      opacity: 0.9,
      backgroundColor: PALETTE[0],
      borderRadius: 8 * s,
      paddingHorizontal: 6 * s,
      paddingVertical: 2 * s,
      elevation: 3 * s,
      shadowColor: 'transparent',
    },
    badgeText: text(9 * s, WHITE, true),
    level: { borderRadius: 4 * s, paddingLeft: 3 * s, paddingTop: s, paddingRight: s, paddingBottom: s, gap: s },
    levelRow: { flexDirection: 'row', alignItems: 'flex-start', gap: 2 * s },
    levelChip: { flex: 1, borderRadius: 2 * s },
    levelText: text(9 * s, WHITE),
  });
}

export default function GauntletApp({ tier = 5, freeze = 0 }: { tier?: number; freeze?: number }) {
  const load = gauntletTier(tier);
  const window = useWindowDimensions();
  const layout = useMemo<Layout>(
    () => ({
      s: load.scale,
      columns: load.columns,
      windowWidth: window.width,
      pixelRatio: PixelRatio.get(),
      styles: createStyles(load.scale),
    }),
    [load, window.width],
  );
  const content = useMemo(
    () => ({
      posts: posts(),
      avatars: Array.from({ length: AVATAR_COUNT }, (_, index) => ({
        uri: pngDataUri(avatarRgba(index), AVATAR_SIZE, AVATAR_SIZE),
      })),
      quotes: tickers(load.tickers),
    }),
    [load],
  );
  return (
    <View style={root.screen}>
      <View style={root.topBar}>
        <Text style={root.title}>Gauntlet</Text>
      </View>
      <View style={root.fill}>
        <WidthFollower layout={layout}>
          <TickerPanel quotes={content.quotes} layout={layout} />
          <View style={stack.area}>
            <CardList tier={load} posts={content.posts} avatars={content.avatars} freeze={freeze} layout={layout} />
            {Array.from({ length: load.layers }, (_, layer) => (
              <StackedLayer key={layer} layer={layer} rows={load.layerRows} />
            ))}
          </View>
        </WidthFollower>
      </View>
    </View>
  );
}

const root = StyleSheet.create({
  screen: { flex: 1, backgroundColor: '#EEF0F5' },
  topBar: { height: 56, backgroundColor: '#1E2A4A', paddingHorizontal: 16, justifyContent: 'center' },
  title: text(20, WHITE, true),
  fill: { flex: 1 },
});

/** The content's width in whole pixels on `frame`. */
function contentWidth(layout: Layout, frame: number): number {
  return Math.round(layout.windowWidth * layout.pixelRatio * widthFraction(frame)) / layout.pixelRatio;
}

/**
 * Lays its children out at the frame's share of the width. The children are
 * elements its parent made once, so the frame re-renders this view alone and
 * Yoga lays everything out again.
 */
function WidthFollower({ layout, children }: { layout: Layout; children: React.ReactNode }) {
  const frame = useFrame();
  const style: ViewStyle = { flex: 1, width: contentWidth(layout, frame) };
  return <View style={style}>{children}</View>;
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
  style: ViewStyle;
}) {
  const frame = useFrame();
  const fill: ViewStyle = { width: `${share(frame) * 100}%`, height: '100%', backgroundColor: color, borderRadius: radius };
  return (
    <View style={[style, { backgroundColor: HAIRLINE, borderRadius: radius }]}>
      <View style={fill} />
    </View>
  );
});

const TickerPanel = memo(function TickerPanel({ quotes, layout }: { quotes: Ticker[]; layout: Layout }) {
  return (
    <View style={layout.styles.panel}>
      {quotes.map((quote, index) => (
        <TickerTile key={quote.symbol + index} quote={quote} index={index} layout={layout} />
      ))}
    </View>
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
    <View style={styles.tile}>
      <Text style={styles.symbol}>{quote.symbol}</Text>
      <TickerPrice quote={quote} layout={layout} />
      <TickerChange quote={quote} layout={layout} />
      <Bar share={share} color={PALETTE[index % PALETTE.length]} radius={2 * s} style={styles.tileBar} />
    </View>
  );
});

/** The price on this frame: its own component, so the frame re-renders only it. */
function TickerPrice({ quote, layout }: { quote: Ticker; layout: Layout }) {
  const frame = useFrame();
  return <Text style={layout.styles.price}>{centsText(tickerCents(quote, frame))}</Text>;
}

/** The signed change, green or red, in its own component like the price. */
function TickerChange({ quote, layout }: { quote: Ticker; layout: Layout }) {
  const frame = useFrame();
  const cents = tickerCents(quote, frame);
  return (
    <Text style={cents >= quote.baseCents ? layout.styles.up : layout.styles.down}>{changeText(quote, cents)}</Text>
  );
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
  avatars: ImageSourcePropType[];
  freeze: number;
  layout: Layout;
}) {
  const list = useRef<FlashListRef<number>>(null);
  const rows = useMemo(() => Array.from({ length: BLOCKS * ROWS_PER_BLOCK }, (_, row) => row), []);

  useEffect(() => {
    let request = 0;
    const tick = () => {
      const frame = clock.advance();
      list.current?.scrollToOffset({ offset: frame * SCROLL_PER_FRAME, animated: false });
      if (freeze > 0 && frame >= freeze) {
        console.log(`PERF frozen frame=${frame}`);
        return;
      }
      request = requestAnimationFrame(tick);
    };
    request = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(request);
  }, [freeze]);

  const renderRow = useCallback<ListRenderItem<number>>(
    ({ item: row }) => {
      const block = Math.trunc(row / ROWS_PER_BLOCK);
      const within = row % ROWS_PER_BLOCK;
      if (within === CARD_ROWS_PER_CLUSTER) {
        return <Level cluster={block} remaining={tier.depth} layout={layout} />;
      }
      const first = (block * CARD_ROWS_PER_CLUSTER + within) * tier.columns;
      return <CardRow first={first} posts={all} avatars={avatars} layout={layout} />;
    },
    [tier, all, avatars, layout],
  );

  const Separator = useMemo(() => () => <View style={layout.styles.separator} />, [layout]);

  return (
    <FlashList
      ref={list}
      data={rows}
      renderItem={renderRow}
      getItemType={row => (row % ROWS_PER_BLOCK === CARD_ROWS_PER_CLUSTER ? 'cluster' : 'cards')}
      contentContainerStyle={layout.styles.list}
      ItemSeparatorComponent={Separator}
    />
  );
});

const CardRow = memo(function CardRow({
  first,
  posts: all,
  avatars,
  layout,
}: {
  first: number;
  posts: Post[];
  avatars: ImageSourcePropType[];
  layout: Layout;
}) {
  const cells = [];
  for (let column = 0; column < layout.columns; column++) {
    const card = first + column;
    cells.push(
      <View key={column} style={layout.styles.cell}>
        <Card post={all[card % POST_COUNT]} avatar={avatars[card % AVATAR_COUNT]} card={card} layout={layout} />
      </View>,
    );
  }
  return <View style={layout.styles.cardRow}>{cells}</View>;
});

const Card = memo(function Card({
  post,
  avatar,
  card,
  layout,
}: {
  post: Post;
  avatar: ImageSourcePropType;
  card: number;
  layout: Layout;
}) {
  const { styles, s } = layout;
  const [tag, tagColor] = post.tags[0];
  const share = useCallback((frame: number) => progressPermille(card, frame) / 1000, [card]);
  return (
    <View>
      <View style={styles.card}>
        <View style={styles.header}>
          <Image source={avatar} style={styles.avatar} />
          <View style={styles.headerText}>
            <Text style={styles.title} numberOfLines={2}>
              {post.title}
            </Text>
            {/* `by Ada Lovelace · #rust`: the author bold, the tag in its color. */}
            <Text style={styles.subtitle} numberOfLines={1}>
              by <Text style={styles.bold}>{post.author}</Text> ·{' '}
              <Text style={{ color: GRADIENT_END[tagColor] }}>{tag}</Text>
            </Text>
          </View>
        </View>
        <Text style={styles.body} numberOfLines={4}>
          {post.body}
        </Text>
        <View style={styles.progressRow}>
          <Bar share={share} color={PALETTE[card % PALETTE.length]} radius={3 * s} style={styles.progressBar} />
          <ProgressLabel card={card} layout={layout} />
        </View>
        <Sparkline card={card} color={PALETTE[post.color]} layout={layout} />
        <View style={styles.chips}>
          {post.tags.map(([chip, color], index) => (
            <View key={index} style={[styles.chip, { backgroundColor: CHIP_BACKGROUND[color] }]}>
              <Text style={[styles.chipText, { color: GRADIENT_END[color] }]}>{chip}</Text>
            </View>
          ))}
        </View>
        <Footer stats={post.stats} layout={layout} />
      </View>
      {card % 5 === 0 ? <Badge card={card} layout={layout} /> : null}
    </View>
  );
});

/** The percent beside the bar, in its own component like the ticker's labels. */
function ProgressLabel({ card, layout }: { card: number; layout: Layout }) {
  const frame = useFrame();
  return <Text style={layout.styles.percent}>{`${Math.trunc(progressPermille(card, frame) / 10)}%`}</Text>;
}

/**
 * A card's 48-point line over a fading fill, drawn from the frame. The chart
 * is as wide as a card's content on this frame, which follows from the
 * frame's width like the rest of the layout.
 */
function Sparkline({ card, color, layout }: { card: number; color: string; layout: Layout }) {
  const frame = useFrame();
  const { s, columns, styles } = layout;
  const inner = contentWidth(layout, frame) - 16 * s;
  const width = (inner - (columns - 1) * 8 * s) / columns - 20 * s;
  const height = 36 * s;
  const { area, line } = useMemo(() => {
    const step = width / (SPARK_POINTS - 1);
    const areaPath = Skia.PathBuilder.Make();
    const linePath = Skia.PathBuilder.Make();
    areaPath.moveTo(0, height);
    for (let index = 0; index < SPARK_POINTS; index++) {
      const x = index * step;
      const y = height * (1 - sparkValue(card, index, frame));
      areaPath.lineTo(x, y);
      if (index === 0) {
        linePath.moveTo(x, y);
      } else {
        linePath.lineTo(x, y);
      }
    }
    areaPath.lineTo(width, height);
    areaPath.close();
    return { area: areaPath.detach(), line: linePath.detach() };
  }, [card, frame, width, height]);
  return (
    <Canvas style={styles.sparkline}>
      <Path path={area}>
        <LinearGradient start={vec(0, 0)} end={vec(0, height)} colors={[`${color}40`, `${color}00`]} />
      </Path>
      <Path path={line} style="stroke" strokeWidth={1.5 * s} color={color} />
    </Canvas>
  );
}

/** Three counters split by dividers as tall as the row: Yoga stretches them. */
const Footer = memo(function Footer({ stats, layout }: { stats: string[]; layout: Layout }) {
  const { styles } = layout;
  return (
    <View style={styles.footer}>
      {FOOTER_LABELS.map((label, index) => (
        <React.Fragment key={label}>
          {index > 0 ? <View style={styles.divider} /> : null}
          <View style={styles.counter}>
            <Text style={styles.stat}>{stats[index]}</Text>
            <Text style={styles.statLabel}>{label}</Text>
          </View>
        </React.Fragment>
      ))}
    </View>
  );
});

/** A translucent tag tilting with the frame. */
function Badge({ card, layout }: { card: number; layout: Layout }) {
  const frame = useFrame();
  const tilt: ViewStyle = { transform: [{ rotate: `${badgeDegrees(card, frame)}deg` }] };
  return (
    <View style={[layout.styles.badge, tilt]}>
      <Text style={layout.styles.badgeText}>HOT</Text>
    </View>
  );
}

/**
 * `depth` levels nested inside one another, each a row of three labels above
 * the next level.
 */
const Level = memo(function Level({ cluster, remaining, layout }: { cluster: number; remaining: number; layout: Layout }) {
  const { styles } = layout;
  return (
    <View style={[styles.level, { backgroundColor: LEVEL_BACKGROUND[remaining % 2] }]}>
      <View style={styles.levelRow}>
        {[0, 1, 2].map(chip => (
          <View
            key={chip}
            style={[styles.levelChip, { backgroundColor: PALETTE[(remaining + chip + cluster) % PALETTE.length] }]}>
            <Text style={styles.levelText}>{`C${cluster}.L${remaining}.${chip}`}</Text>
          </View>
        ))}
      </View>
      {remaining > 0 ? <Level cluster={cluster} remaining={remaining - 1} layout={layout} /> : null}
    </View>
  );
});

const stack = StyleSheet.create({
  // The panels show only over the list, as every app clips them.
  area: { flex: 1, overflow: 'hidden' },
  layer: {
    position: 'absolute',
    left: 0,
    top: 0,
    width: 220,
    padding: 10,
    gap: 8,
    borderRadius: 12,
    backgroundColor: '#1E293BC0',
  },
  title: text(13, WHITE, true),
  rows: { gap: 3 },
  row: { flexDirection: 'row', gap: 3 },
  cell: { width: 22, height: 22, borderRadius: 4, alignItems: 'center', justifyContent: 'center' },
  cellText: text(9, WHITE),
  nested: { padding: 8, gap: 2, borderRadius: 8, backgroundColor: '#FFFFFFE6' },
  nestedTitle: text(11, INK, true),
  nestedText: text(11, BODY),
});

/**
 * A translucent panel stacked over the list. Its content is elements made
 * once, so each frame re-renders only the two views that carry the panel's
 * and its nested card's transforms.
 */
const StackedLayer = memo(function StackedLayer({ layer, rows }: { layer: number; rows: number }) {
  return (
    <LayerTilt layer={layer}>
      <Text style={stack.title}>{`Layer ${layer + 1}`}</Text>
      <View style={stack.rows}>
        {Array.from({ length: rows }, (_, row) => (
          <LayerCells key={row} layer={layer} row={row} />
        ))}
      </View>
      <NestedTilt layer={layer}>
        <Text style={stack.nestedTitle}>{`Nested in layer ${layer + 1}`}</Text>
        <Text style={stack.nestedText}>Tilts against its panel</Text>
      </NestedTilt>
    </LayerTilt>
  );
});

/** The panel moved and tilted on this frame. */
function LayerTilt({ layer, children }: { layer: number; children: React.ReactNode }) {
  const frame = useFrame();
  const place: ViewStyle = {
    transform: [
      { translateX: layerX(layer, frame) },
      { translateY: layerY(layer, frame) },
      { rotate: `${layerDegrees(layer, frame)}deg` },
    ],
  };
  return <View style={[stack.layer, place]}>{children}</View>;
}

/** The nested card tilted against its panel on this frame. */
function NestedTilt({ layer, children }: { layer: number; children: React.ReactNode }) {
  const frame = useFrame();
  const tilt: ViewStyle = { transform: [{ rotate: `${-layerDegrees(layer, frame)}deg` }] };
  return <View style={[stack.nested, tilt]}>{children}</View>;
}

/** One row of a stacked panel's numbered cells. */
const LayerCells = memo(function LayerCells({ layer, row }: { layer: number; row: number }) {
  return (
    <View style={stack.row}>
      {Array.from({ length: LAYER_COLUMNS }, (_, column) => {
        const cell = row * LAYER_COLUMNS + column;
        return (
          <View key={column} style={[stack.cell, { backgroundColor: PALETTE[layerCellColor(layer, cell)] }]}>
            <Text style={stack.cellText}>{cell + 1}</Text>
          </View>
        );
      })}
    </View>
  );
});
