// The still wall in Flutter (`../../README.md`): a window of tiles that never
// change, under a clock line whose text changes every frame, with a cursor
// outline that rests on another tile every frame. Its data follows
// `perf-data/src/wall.rs` bit for bit.
//
// In Flutter's idiom a ticker advances the frame in a `ValueNotifier`. The
// tiles are built once, below a `LayoutBuilder` that builds again only when
// the wall's size changes, inside a `RepaintBoundary` that keeps their
// picture. The clock's text and the cursor's offset are the only widgets that
// listen to the frame.

import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'data.dart';
import 'host.dart';

/// One load tier of the still wall: the size of its tiles, in whole points.
class WallTier {
  final double tileWidth;
  final double tileHeight;

  const WallTier(this.tileWidth, this.tileHeight);
}

/// The tiers, 1 to 20.
const wallTiers = [
  WallTier(64, 40), WallTier(54, 34), WallTier(46, 29), WallTier(40, 25), WallTier(34, 21),
  WallTier(30, 18), WallTier(26, 16), WallTier(22, 14), WallTier(19, 12), WallTier(16, 10),
  WallTier(14, 9), WallTier(12, 8), WallTier(10, 7), WallTier(8, 6), WallTier(7, 5),
  WallTier(6, 4), WallTier(5, 4), WallTier(5, 3), WallTier(4, 3), WallTier(3, 3),
];

/// Tier 1 to 20; anything else is clamped into that range.
WallTier wallTier(int tier) => wallTiers[tier.clamp(1, wallTiers.length) - 1];

const wallGap = 1.0;
const wallClockHeight = 24.0;
const wallClockText = 14.0;
const wallCorner = 2.0;
const wallCursorWidth = 2.0;
const wallBackground = Color(0xFF0F1220);
const _ink = Color(0xFFFFFFFF);

/// The tiles across and down that fill a wall [width] by [height] points.
(int, int) wallGrid(WallTier tier, double width, double height) {
  int fit(double space, double tile) => math.max(1, ((space - wallGap) / (tile + wallGap)).floor());
  return (fit(width, tier.tileWidth), fit(height, tier.tileHeight));
}

String wallLabel(int index) => index.toString().padLeft(5, '0');

String wallValue(int index) => ((((index * 2654435761) & 0xFFFFFFFF) >> 22) % 1000).toString().padLeft(3, '0');

int wallFillArgb(int index) => paletteArgb[index % paletteArgb.length];

int wallDotArgb(int index) => chipBackgroundArgb[(index ~/ paletteArgb.length) % chipBackgroundArgb.length];

int wallCursor(int frame, int tiles) => (frame * 37) % math.max(tiles, 1);

String wallClock(int frame) => 'frame ${frame.toString().padLeft(6, '0')}';

/// White text of [size], bold or not.
TextStyle inkText(double size, {bool bold = false}) =>
    TextStyle(fontSize: size, color: _ink, fontWeight: bold ? FontWeight.bold : FontWeight.normal);

/// The clock line: its text is the only widget that listens to [frame].
class WallClock extends StatelessWidget {
  final ValueListenable<int> frame;

  const WallClock(this.frame, {super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      height: wallClockHeight,
      padding: const EdgeInsets.symmetric(horizontal: 8),
      alignment: Alignment.centerLeft,
      child: ValueListenableBuilder(
        valueListenable: frame,
        builder: (context, frame, _) => Text(wallClock(frame), style: inkText(wallClockText)),
      ),
    );
  }
}

/// The clock line, the tiles and the cursor, under the top bar.
class Wall extends StatefulWidget {
  final WallTier tier;

  /// Stop on this frame and hold still, for picture comparisons; 0 runs on.
  final int freeze;

  const Wall(this.tier, this.freeze, {super.key});

  @override
  State<Wall> createState() => _WallState();
}

class _WallState extends State<Wall> with SingleTickerProviderStateMixin {
  final _frame = ValueNotifier(0);
  late final _ticker = createTicker(_advance);

  @override
  void initState() {
    super.initState();
    _ticker.start();
  }

  void _advance(Duration elapsed) {
    final index = _frame.value + 1;
    _frame.value = index;
    final freeze = widget.freeze;
    if (freeze > 0 && index >= freeze) {
      _ticker.stop();
      perf('PERF frozen frame=$index');
    }
  }

  @override
  void dispose() {
    _ticker.dispose();
    _frame.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tier = widget.tier;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        WallClock(_frame),
        Expanded(
          child: LayoutBuilder(builder: (context, constraints) {
            final (columns, rows) = wallGrid(tier, constraints.maxWidth, constraints.maxHeight);
            return Stack(
              children: [
                RepaintBoundary(child: WallTiles(columns, rows, tier)),
                _Cursor(_frame, columns, columns * rows, tier),
              ],
            );
          }),
        ),
      ],
    );
  }
}

/// Rows of cells, [wallGap] apart and from the edges.
class WallCells extends StatelessWidget {
  final List<Widget> rows;

  const WallCells(this.rows, {super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(wallGap),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, spacing: wallGap, children: rows),
    );
  }
}

/// The tiles, [columns] by [rows] of them, `wallGap` apart.
class WallTiles extends StatelessWidget {
  final int columns;
  final int rows;
  final WallTier tier;

  const WallTiles(this.columns, this.rows, this.tier, {super.key});

  @override
  Widget build(BuildContext context) {
    return WallCells([
      for (var row = 0; row < rows; row++)
        Row(
          spacing: wallGap,
          children: [
            for (var column = 0; column < columns; column++) WallTile(row * columns + column, tier),
          ],
        ),
    ]);
  }
}

/// One tile: its label over a dot and its value, on its fill. [width],
/// [height] and [fill] replace the tile's box and its fill.
class WallTile extends StatelessWidget {
  final int index;
  final WallTier tier;
  final double? width;
  final double? height;
  final Color? fill;

  const WallTile(this.index, this.tier, {this.width, this.height, this.fill, super.key});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: width ?? tier.tileWidth,
      height: height ?? tier.tileHeight,
      decoration: BoxDecoration(
          color: fill ?? Color(wallFillArgb(index)), borderRadius: BorderRadius.circular(wallCorner)),
      child: WallTileBody(index, tier),
    );
  }
}

/// A tile's label, dot and value, without its fill: the text and the padding
/// follow the tier's tile.
class WallTileBody extends StatelessWidget {
  final int index;
  final WallTier tier;

  const WallTileBody(this.index, this.tier, {super.key});

  @override
  Widget build(BuildContext context) {
    final height = tier.tileHeight;
    final dot = height * 0.22;
    return Padding(
      padding: EdgeInsets.all(height * 0.06),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(wallLabel(index),
              style: inkText(height * 0.30, bold: true), softWrap: false, overflow: TextOverflow.clip, maxLines: 1),
          Row(
            spacing: dot * 0.5,
            children: [
              Container(
                width: dot,
                height: dot,
                decoration: BoxDecoration(
                    color: Color(wallDotArgb(index)), borderRadius: BorderRadius.circular(dot * 0.5)),
              ),
              Flexible(
                child: Text(wallValue(index),
                    style: inkText(height * 0.26), softWrap: false, overflow: TextOverflow.clip, maxLines: 1),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

/// The cursor's outline, over the tile `wallCursor` names for the frame: it
/// listens to the frame and moves by its offset alone.
class _Cursor extends StatelessWidget {
  final ValueNotifier<int> frame;
  final int columns;
  final int tiles;
  final WallTier tier;

  const _Cursor(this.frame, this.columns, this.tiles, this.tier);

  @override
  Widget build(BuildContext context) {
    final (width, height) = (tier.tileWidth, tier.tileHeight);
    final outline = SizedBox(
      width: width,
      height: height,
      child: DecoratedBox(
        decoration: BoxDecoration(
          border: Border.all(color: _ink, width: wallCursorWidth),
          borderRadius: BorderRadius.circular(wallCorner),
        ),
      ),
    );
    return ValueListenableBuilder(
      valueListenable: frame,
      child: outline,
      builder: (context, frame, child) {
        final tile = wallCursor(frame, tiles);
        return Transform.translate(
          offset: Offset(wallGap + (tile % columns) * (width + wallGap), wallGap + (tile ~/ columns) * (height + wallGap)),
          child: child,
        );
      },
    );
  }
}
