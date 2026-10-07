// The gauntlet in NativeScript: the screen `compose-app/.../Gauntlet.kt`
// draws, element for element, in NativeScript's own idiom: its core layouts
// and labels, which are Android views driven from JavaScript, a ListView
// whose two row templates recycle each kind of row, and an Android view of
// the app's own, written in TypeScript, for the sparklines. The benchmark's
// README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. A Choreographer frame callback advances it once per frame;
// the views on screen set only what changed.

import {
  Application,
  Color,
  File,
  FlexboxLayout,
  FormattedString,
  GridLayout,
  Image,
  ImageSource,
  ItemSpec,
  Label,
  ListView,
  Screen,
  Span,
  StackLayout,
  Utils,
  View,
  knownFolders,
} from '@nativescript/core';
import {
  AVATAR_COUNT,
  AVATAR_SIZE,
  CARD_ROWS_PER_CLUSTER,
  CHIP_BACKGROUND,
  GRADIENT_END,
  LAYER_COLUMNS,
  PALETTE,
  POST_COUNT,
  Post,
  SPARK_POINTS,
  Ticker,
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

/** NativeScript's runtime: ties a JavaScript object extending a Java class to its Java object. */
declare const global: { __native<T>(object: T): T };

/** Blocks of five card rows and a cluster: no measurement window reaches the end. */
const ROWS = 2000 * (CARD_ROWS_PER_CLUSTER + 1);
/** How far the list scrolls each frame, in dp. */
const SCROLL_PER_FRAME = 3;
const FOOTER_LABELS = ['likes', 'replies', 'shares'];
const INK = new Color('#111827');
const BODY = new Color('#374151');
const MUTED = new Color('#6B7280');
const HAIRLINE = new Color('#E5E7EB');
const WHITE = new Color('#FFFFFF');
const UP = new Color('#16A34A');
const DOWN = new Color('#DC2626');
const LEVEL_BACKGROUND = [new Color('#F1F5F9'), new Color('#CBD5E1')];
const LAYER_BACKGROUND = new Color(0xc0, 0x1e, 0x29, 0x3b);
const NESTED_BACKGROUND = new Color(0xe6, 0xff, 0xff, 0xff);
const PALETTE_COLORS = PALETTE.map((color) => new Color(color));
const CHIP_COLORS = CHIP_BACKGROUND.map((color) => new Color(color));
const GRADIENT_COLORS = GRADIENT_END.map((color) => new Color(color));

/** Writes a `PERF` line under the log tag `measure.py` reads. */
function log(line: string) {
  android.util.Log.i('PerfCompare', line);
}

/** The frame index every per-frame value follows, and the views it updates. */
const clock = {
  frame: 0,
  followers: new Set<(frame: number) => void>(),
  /** Calls `onFrame` every frame while `view` is on screen. */
  follow(view: View, onFrame: (frame: number) => void) {
    view.on(View.loadedEvent, () => clock.followers.add(onFrame));
    view.on(View.unloadedEvent, () => clock.followers.delete(onFrame));
  },
};

/** The device's Roboto faces, the files the other apps load, as `fontFamily` names. */
const ROBOTO = 'Roboto-Regular';
const ROBOTO_BOLD = 'Roboto-Bold';

/** Copies the device's Roboto files into the app's `fonts` folder, where
 * NativeScript's `fontFamily` looks: the system's own sans-serif is not Roboto
 * on every phone. */
function installRoboto() {
  const fonts = knownFolders.currentApp().getFolder('fonts');
  for (const face of [ROBOTO, ROBOTO_BOLD]) {
    fonts.getFile(`${face}.ttf`).writeSync(File.fromPath(`/system/fonts/${face}.ttf`).readSync());
  }
}

/** Text in Roboto at one tier's scale. */
function text(size: number, s: number, color: Color, bold = false): Label {
  const label = new Label();
  label.fontSize = size * s;
  label.color = color;
  label.fontFamily = bold ? ROBOTO_BOLD : ROBOTO;
  label.fontWeight = bold ? 'bold' : 'normal';
  return label;
}

/** Lines 1.4 em apart, cut with an ellipsis after `lines`: NativeScript's
 * line height adds to Roboto's own 1.172 em. */
function paragraph(size: number, s: number, color: Color, lines: number, bold = false): Label {
  const label = text(size, s, color, bold);
  label.textWrap = true;
  label.maxLines = lines;
  label.lineHeight = size * s * (1.4 - 1.172);
  return label;
}

/** A rounded track filled to a share: the fill is a percentage of its width. */
class Bar extends GridLayout {
  private readonly fill = new StackLayout();

  constructor(radius: number) {
    super();
    this.backgroundColor = HAIRLINE;
    this.borderRadius = radius;
    this.fill.borderRadius = radius;
    this.fill.horizontalAlignment = 'left';
    this.addChild(this.fill);
  }

  paint(color: Color) {
    this.fill.backgroundColor = color;
  }

  set share(share: number) {
    this.fill.width = { unit: '%', value: Math.min(Math.max(share, 0), 1) };
  }
}

function tickerTile(ticker: Ticker, index: number, s: number): View {
  const tile = new StackLayout();
  tile.orientation = 'horizontal';
  tile.backgroundColor = WHITE;
  tile.borderRadius = 6 * s;
  tile.padding = `${3 * s} ${6 * s}`;
  const symbol = text(10, s, INK, true);
  symbol.text = ticker.symbol;
  const price = text(10, s, BODY);
  const change = text(10, s, UP);
  const bar = new Bar(2 * s);
  bar.width = 20 * s;
  bar.height = 4 * s;
  bar.paint(PALETTE_COLORS[index % PALETTE_COLORS.length]);
  for (const view of [symbol, price, change, bar] as View[]) {
    view.verticalAlignment = 'middle';
    view.marginRight = 4 * s;
    tile.addChild(view);
  }
  bar.marginRight = 0;
  const update = (frame: number) => {
    const cents = tickerCents(ticker, frame);
    price.text = centsText(cents);
    change.text = changeText(ticker, cents);
    change.color = cents >= ticker.baseCents ? UP : DOWN;
    bar.share = (cents - ticker.baseCents + ticker.swingCents) / (2 * ticker.swingCents);
  };
  update(clock.frame);
  clock.follow(tile, update);
  return tile;
}

/** The Android view that draws a card's 48-point line over a fading fill. */
@NativeClass()
class SparklineView extends android.view.View {
  card = 0;
  private readonly line = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
  private readonly fill = new android.graphics.Paint(android.graphics.Paint.ANTI_ALIAS_FLAG);
  private readonly area = new android.graphics.Path();
  private readonly stroke = new android.graphics.Path();
  private argb = 0;
  private shaderHeight = -1;

  constructor(context: android.content.Context, strokeWidth: number) {
    super(context);
    this.line.setStyle(android.graphics.Paint.Style.STROKE);
    this.line.setStrokeWidth(strokeWidth);
    return global.__native(this);
  }

  bind(card: number, argb: number) {
    this.card = card;
    if (this.argb !== argb) {
      this.argb = argb;
      this.line.setColor(argb);
      this.shaderHeight = -1;
    }
  }

  onDraw(canvas: android.graphics.Canvas) {
    const frame = clock.frame;
    const width = this.getWidth();
    const height = this.getHeight();
    if (this.shaderHeight !== height) {
      this.shaderHeight = height;
      const rgb = this.argb & 0xffffff;
      this.fill.setShader(new android.graphics.LinearGradient(0, 0, 0, height, (0x40 << 24) | rgb, rgb,
        android.graphics.Shader.TileMode.CLAMP));
    }
    const step = width / (SPARK_POINTS - 1);
    this.area.rewind();
    this.stroke.rewind();
    this.area.moveTo(0, height);
    for (let index = 0; index < SPARK_POINTS; index++) {
      const x = index * step;
      const y = height * (1 - sparkValue(this.card, index, frame));
      this.area.lineTo(x, y);
      if (index === 0) this.stroke.moveTo(x, y);
      else this.stroke.lineTo(x, y);
    }
    this.area.lineTo(width, height);
    this.area.close();
    canvas.drawPath(this.area, this.fill);
    canvas.drawPath(this.stroke, this.line);
  }
}

/** The sparkline as a NativeScript view. */
class Sparkline extends View {
  constructor(private readonly strokeWidth: number) {
    super();
  }

  createNativeView() {
    return new SparklineView(this._context, this.strokeWidth);
  }

  get spark(): SparklineView {
    return this.nativeViewProtected as SparklineView;
  }
}

function argbOf(hex: string): number {
  return (0xff000000 | parseInt(hex.slice(1), 16)) | 0;
}

/** One post: header, body, progress, sparkline, chips and footer, and every fifth card a tilting badge. */
class CardView extends GridLayout {
  private readonly avatar = new Image();
  private readonly title: Label;
  private readonly author = new Span();
  private readonly tag = new Span();
  private readonly body: Label;
  private readonly progress: Bar;
  private readonly percent: Label;
  private readonly sparkline: Sparkline;
  private readonly chips: Label[] = [];
  private readonly stats: Label[] = [];
  private readonly badge: Label;
  private card = 0;
  private post?: Post;

  constructor(private readonly s: number) {
    super();
    this.avatar.width = 32 * s;
    this.avatar.height = 32 * s;
    this.avatar.borderRadius = 16 * s;
    this.avatar.stretch = 'aspectFill';
    this.avatar.verticalAlignment = 'middle';
    this.avatar.marginRight = 8 * s;
    this.title = paragraph(13, s, INK, 2, true);
    const subtitle = text(11, s, MUTED);
    subtitle.maxLines = 1;
    // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
    const spans = new FormattedString();
    const by = new Span();
    by.text = 'by ';
    this.author.fontFamily = ROBOTO_BOLD;
    this.author.fontWeight = 'bold';
    const dot = new Span();
    dot.text = ' · ';
    spans.spans.push(by, this.author, dot, this.tag);
    subtitle.formattedText = spans;
    const headText = new StackLayout();
    headText.verticalAlignment = 'middle';
    headText.addChild(this.title);
    headText.addChild(subtitle);
    const header = new GridLayout();
    header.addColumn(new ItemSpec(1, 'auto'));
    header.addColumn(new ItemSpec(1, 'star'));
    header.addChild(this.avatar);
    GridLayout.setColumn(headText, 1);
    header.addChild(headText);

    this.body = paragraph(12, s, BODY, 4);

    this.progress = new Bar(3 * s);
    this.progress.height = 6 * s;
    this.progress.verticalAlignment = 'middle';
    this.percent = text(10, s, MUTED);
    this.percent.verticalAlignment = 'middle';
    this.percent.marginLeft = 6 * s;
    const progressRow = new GridLayout();
    progressRow.addColumn(new ItemSpec(1, 'star'));
    progressRow.addColumn(new ItemSpec(1, 'auto'));
    progressRow.addChild(this.progress);
    GridLayout.setColumn(this.percent, 1);
    progressRow.addChild(this.percent);

    this.sparkline = new Sparkline(1.5 * s * Screen.mainScreen.scale);
    this.sparkline.height = 36 * s;

    const chipFlow = new FlexboxLayout();
    chipFlow.flexWrap = 'wrap';
    for (let index = 0; index < 3; index++) {
      const chip = text(10, s, INK);
      chip.padding = `${3 * s} ${8 * s}`;
      chip.borderRadius = 10 * s;
      chip.marginRight = 4 * s;
      chip.marginBottom = 4 * s;
      chipFlow.addChild(chip);
      this.chips.push(chip);
    }

    // Three counters split by dividers as tall as the tallest counter: the
    // grid's row is as tall as its tallest cell.
    const footer = new GridLayout();
    FOOTER_LABELS.forEach((name, index) => {
      if (index > 0) {
        footer.addColumn(new ItemSpec(1, 'pixel'));
        const divider = new StackLayout();
        divider.backgroundColor = HAIRLINE;
        GridLayout.setColumn(divider, index * 2 - 1);
        footer.addChild(divider);
      }
      footer.addColumn(new ItemSpec(1, 'star'));
      const counter = new StackLayout();
      const stat = text(12, s, INK, true);
      stat.horizontalAlignment = 'center';
      const label = text(9, s, MUTED);
      label.text = name;
      label.horizontalAlignment = 'center';
      counter.addChild(stat);
      counter.addChild(label);
      GridLayout.setColumn(counter, index * 2);
      footer.addChild(counter);
      this.stats.push(stat);
    });

    const column = new StackLayout();
    column.backgroundColor = WHITE;
    column.borderRadius = 12 * s;
    column.borderWidth = 1;
    column.borderColor = HAIRLINE;
    column.padding = 10 * s;
    column.boxShadow = `0 ${s} ${3 * s} rgba(0, 0, 0, 0.24)`;
    const pieces: View[] = [header, this.body, progressRow, this.sparkline, chipFlow, footer];
    pieces.forEach((piece, index) => {
      if (index > 0) piece.marginTop = 6 * s;
      column.addChild(piece);
    });
    this.addChild(column);

    this.badge = text(9, s, WHITE, true);
    this.badge.text = 'HOT';
    this.badge.backgroundColor = PALETTE_COLORS[0];
    this.badge.borderRadius = 8 * s;
    this.badge.padding = `${2 * s} ${6 * s}`;
    this.badge.horizontalAlignment = 'right';
    this.badge.verticalAlignment = 'top';
    this.badge.margin = 6 * s;
    this.badge.opacity = 0.9;
    this.addChild(this.badge);
    clock.follow(this, (frame) => this.update(frame));
  }

  show(card: number, post: Post, avatar: ImageSource) {
    this.card = card;
    this.post = post;
    this.avatar.src = avatar;
    this.title.text = post.title;
    this.author.text = post.author;
    const [firstTag, tagColor] = post.tags[0];
    this.tag.text = firstTag;
    this.tag.color = GRADIENT_COLORS[tagColor];
    this.body.text = post.body;
    this.progress.paint(PALETTE_COLORS[card % PALETTE_COLORS.length]);
    post.tags.forEach(([chip, color], index) => {
      this.chips[index].text = chip;
      this.chips[index].backgroundColor = CHIP_COLORS[color];
      this.chips[index].color = GRADIENT_COLORS[color];
    });
    this.stats.forEach((stat, index) => (stat.text = post.stats[index]));
    this.badge.visibility = card % 5 === 0 ? 'visible' : 'collapse';
    this.update(clock.frame);
  }

  private update(frame: number) {
    const permille = progressPermille(this.card, frame);
    this.percent.text = `${Math.trunc(permille / 10)}%`;
    this.progress.share = permille / 1000;
    const spark = this.sparkline.spark;
    if (spark && this.post) {
      spark.bind(this.card, argbOf(PALETTE[this.post.color]));
      spark.invalidate();
    }
    if (this.card % 5 === 0) this.badge.rotate = badgeDegrees(this.card, frame);
  }
}

/** A row of cards, rebound as the list reuses it. */
class CardRow extends GridLayout {
  readonly cards: CardView[] = [];

  constructor(columns: number, s: number) {
    super();
    this.padding = `0 ${8 * s} ${8 * s} ${8 * s}`;
    for (let column = 0; column < columns; column++) {
      if (column > 0) this.addColumn(new ItemSpec(8 * s, 'pixel'));
      this.addColumn(new ItemSpec(1, 'star'));
      const card = new CardView(s);
      card.verticalAlignment = 'top';
      GridLayout.setColumn(card, column * 2);
      this.addChild(card);
      this.cards.push(card);
    }
  }
}

/** `depth` levels nested inside one another, each a row of three labels above the next level. */
class Level {
  readonly view = new StackLayout();
  private readonly chips: Label[] = [];
  private readonly next?: Level;

  constructor(private readonly remaining: number, s: number) {
    const row = new GridLayout();
    for (let chip = 0; chip < 3; chip++) {
      if (chip > 0) row.addColumn(new ItemSpec(2 * s, 'pixel'));
      row.addColumn(new ItemSpec(1, 'star'));
      const label = text(9, s, WHITE);
      label.borderRadius = 2 * s;
      label.verticalAlignment = 'top';
      GridLayout.setColumn(label, chip * 2);
      row.addChild(label);
      this.chips.push(label);
    }
    this.view.backgroundColor = LEVEL_BACKGROUND[remaining % 2];
    this.view.borderRadius = 4 * s;
    this.view.padding = `${s} ${s} ${s} ${3 * s}`;
    this.view.addChild(row);
    if (remaining > 0) {
      this.next = new Level(remaining - 1, s);
      this.next.view.marginTop = s;
      this.view.addChild(this.next.view);
    }
  }

  bind(cluster: number) {
    this.chips.forEach((chip, index) => {
      chip.text = `C${cluster}.L${this.remaining}.${index}`;
      chip.backgroundColor = PALETTE_COLORS[(this.remaining + index + cluster) % PALETTE_COLORS.length];
    });
    this.next?.bind(cluster);
  }
}

class ClusterRow extends StackLayout {
  readonly level: Level;

  constructor(depth: number, s: number) {
    super();
    this.padding = `0 ${8 * s} ${8 * s} ${8 * s}`;
    this.level = new Level(depth, s);
    this.addChild(this.level.view);
  }
}

function avatarSource(index: number): ImageSource {
  const rgba = avatarRgba(index);
  const pixels = Array.create('int', AVATAR_SIZE * AVATAR_SIZE);
  for (let pixel = 0; pixel < AVATAR_SIZE * AVATAR_SIZE; pixel++) {
    const at = pixel * 4;
    pixels[pixel] = ((0xff << 24) | (rgba[at] << 16) | (rgba[at + 1] << 8) | rgba[at + 2]) | 0;
  }
  const bitmap = android.graphics.Bitmap.createBitmap(pixels, AVATAR_SIZE, AVATAR_SIZE,
    android.graphics.Bitmap.Config.ARGB_8888);
  return new ImageSource(bitmap);
}

/** A translucent panel stacked over the list. Its views never change: each
 * frame sets only the panel's translation and rotation and its nested card's
 * rotation, which Android applies to their render nodes. */
function stackedLayer(layer: number, rows: number): View {
  const panel = new StackLayout();
  panel.width = 220;
  panel.horizontalAlignment = 'left';
  panel.verticalAlignment = 'top';
  panel.padding = 10;
  panel.borderRadius = 12;
  panel.backgroundColor = LAYER_BACKGROUND;
  const title = text(13, 1, WHITE, true);
  title.text = `Layer ${layer + 1}`;
  panel.addChild(title);
  for (let row = 0; row < rows; row++) {
    const cells = new StackLayout();
    cells.orientation = 'horizontal';
    cells.marginTop = row === 0 ? 8 : 3;
    for (let column = 0; column < LAYER_COLUMNS; column++) {
      const cell = row * LAYER_COLUMNS + column;
      const box = new GridLayout();
      box.width = 22;
      box.height = 22;
      box.marginLeft = column === 0 ? 0 : 3;
      box.borderRadius = 4;
      box.backgroundColor = PALETTE_COLORS[layerCellColor(layer, cell)];
      const label = text(9, 1, WHITE);
      label.text = `${cell + 1}`;
      label.horizontalAlignment = 'center';
      label.verticalAlignment = 'middle';
      box.addChild(label);
      cells.addChild(box);
    }
    panel.addChild(cells);
  }
  const nested = new StackLayout();
  nested.marginTop = 8;
  nested.padding = 8;
  nested.borderRadius = 8;
  nested.backgroundColor = NESTED_BACKGROUND;
  const heading = text(11, 1, INK, true);
  heading.text = `Nested in layer ${layer + 1}`;
  const line = text(11, 1, BODY);
  line.text = 'Tilts against its panel';
  line.marginTop = 2;
  nested.addChild(heading);
  nested.addChild(line);
  panel.addChild(nested);
  const place = (frame: number) => {
    const degrees = layerDegrees(layer, frame);
    panel.translateX = layerX(layer, frame);
    panel.translateY = layerY(layer, frame);
    panel.rotate = degrees;
    nested.rotate = -degrees;
  };
  place(0);
  clock.follow(panel, place);
  return panel;
}

function gauntlet(): View {
  const intent = Application.android.startActivity?.getIntent();
  const tierIndex = intent?.getIntExtra('tier', 5) ?? 5;
  const freeze = intent?.getIntExtra('freeze', 0) ?? 0;
  const tier = gauntletTier(tierIndex);
  const s = tier.scale;
  const all = posts();
  const avatars = Array.from({ length: AVATAR_COUNT }, (_, index) => avatarSource(index));

  const root = new GridLayout();
  root.backgroundColor = new Color('#EEF0F5');
  root.addRow(new ItemSpec(56, 'pixel'));
  root.addRow(new ItemSpec(1, 'star'));
  const topBar = new GridLayout();
  topBar.backgroundColor = new Color('#1E2A4A');
  topBar.padding = '0 16';
  const title = text(20, 1, WHITE, true);
  title.text = 'Gauntlet';
  title.verticalAlignment = 'middle';
  topBar.addChild(title);
  root.addChild(topBar);

  const panel = new FlexboxLayout();
  panel.flexWrap = 'wrap';
  panel.backgroundColor = new Color('#E2E8F0');
  panel.padding = `${6 * s} ${2 * s} ${2 * s} ${6 * s}`;
  tickers(tier.tickers).forEach((ticker, index) => {
    const tile = tickerTile(ticker, index, s);
    tile.marginRight = 4 * s;
    tile.marginBottom = 4 * s;
    panel.addChild(tile);
  });

  const list = new ListView();
  list.items = Array.from({ length: ROWS }, (_, index) => index);
  list.separatorColor = new Color('transparent');
  const isCluster = (row: number) => row % (CARD_ROWS_PER_CLUSTER + 1) === CARD_ROWS_PER_CLUSTER;
  list.itemTemplateSelector = (row: number) => (isCluster(row) ? 'cluster' : 'cards');
  list.itemTemplates = [
    { key: 'cards', createView: () => new CardRow(tier.columns, s) },
    { key: 'cluster', createView: () => new ClusterRow(tier.depth, s) },
  ];
  list.on(ListView.itemLoadingEvent, (event: any) => {
    const row: number = event.index;
    // The list's padding above its first row, which scrolls with the rows.
    (event.view as StackLayout).paddingTop = row === 0 ? 8 * s : 0;
    const block = Math.trunc(row / (CARD_ROWS_PER_CLUSTER + 1));
    const within = row % (CARD_ROWS_PER_CLUSTER + 1);
    if (event.view instanceof ClusterRow) {
      event.view.level.bind(block);
    } else if (event.view instanceof CardRow) {
      const first = (block * CARD_ROWS_PER_CLUSTER + within) * tier.columns;
      event.view.cards.forEach((card: CardView, column: number) => {
        const index = first + column;
        card.show(index, all[index % POST_COUNT], avatars[index % AVATAR_COUNT]);
      });
    }
  });

  const content = new GridLayout();
  content.horizontalAlignment = 'left';
  content.addRow(new ItemSpec(1, 'auto'));
  content.addRow(new ItemSpec(1, 'star'));
  content.addChild(panel);
  // The panels stack over the list and show only over it, as every app clips them.
  const area = new GridLayout();
  area.clipToBounds = true;
  area.addChild(list);
  for (let layer = 0; layer < tier.layers; layer++) area.addChild(stackedLayer(layer, tier.layerRows));
  GridLayout.setRow(area, 1);
  content.addChild(area);
  GridLayout.setRow(content, 1);
  root.addChild(content);

  // Android's Choreographer drives the frames, as in the Views app.
  // NativeScript's requestAnimationFrame runs from the native choreographer's
  // vsync callback, which the main thread serves before its Java messages:
  // once a frame takes longer than a vsync, each tick blocks the Java message
  // queue again, and the window never reports its first draw.
  const choreographer = android.view.Choreographer.getInstance();
  const frameCallback = new android.view.Choreographer.FrameCallback({ doFrame: () => tick() });
  // The list scrolls by whole pixels; the remainder carries to the next frame.
  let scrolled = 0;
  const tick = () => {
    if (clock.frame === 0) log('PERF first_frame');
    clock.frame += 1;
    const frame = clock.frame;
    const widthPixels = Screen.mainScreen.widthPixels;
    content.width = Utils.layout.toDeviceIndependentPixels(Math.round(widthPixels * widthFraction(frame)));
    clock.followers.forEach((onFrame) => onFrame(frame));
    const target = Math.round(Utils.layout.toDevicePixels(frame * SCROLL_PER_FRAME));
    const native = list.android as android.widget.ListView | undefined;
    if (native) {
      native.scrollListBy(target - scrolled);
      scrolled = target;
    }
    if (freeze > 0 && frame >= freeze) {
      log(`PERF frozen frame=${frame}`);
      return;
    }
    choreographer.postFrameCallback(frameCallback);
  };
  root.on(View.loadedEvent, () => choreographer.postFrameCallback(frameCallback));
  return root;
}

installRoboto();
Application.run({ create: gauntlet });
