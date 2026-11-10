// The patchwork in Flutter (`../../README.md`): nine patches of the still
// wall's tiles beside a chat. Two patches never change; in the others a
// frame changes a small part. Its data follows `perf-data/src/patchwork.rs`
// bit for bit.
//
// In Flutter's idiom a ticker advances the frame in a `ValueNotifier` and
// every changing part listens to a value derived from it (`FrameSelect`)
// that changes only when the part does, so a frame builds again the row, the
// card, the two items or the digits that changed. Repaint boundaries keep
// the pictures of what stayed: each patch, the charts' painter that never
// repaints, the text of a recolor row and the boxes of a deep row under the
// fills and digits that change, each row of the scroll list, each tile of
// the reorder and pile patches. The cards are boundaries under a
// `Transform`, which moves them without painting them again. The paragraph
// is one `Text`, which Flutter lays out whole.

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'chat.dart';
import 'chat_data.dart';
import 'chat_feed.dart';
import 'frame_select.dart';
import 'host.dart';
import 'patchwork_data.dart';
import 'wall.dart';

/// One patch: its tiles and its size in points.
class _Grid {
  final PatchTiles tiles;
  final double width;
  final double height;

  const _Grid(this.tiles, this.width, this.height);

  WallTier get tier => tiles.tier;
}

class Patchwork extends StatefulWidget {
  final WallTier tier;

  /// Stop on this frame and hold still, for picture comparisons; 0 runs on.
  final int freeze;
  final ChatAssets assets;
  final ChatFeed feed;

  const Patchwork(this.tier, this.freeze, this.assets, this.feed, {super.key});

  @override
  State<Patchwork> createState() => _PatchworkState();
}

class _PatchworkState extends State<Patchwork> with SingleTickerProviderStateMixin {
  final _frame = ValueNotifier(0);
  late final _ticker = createTicker(_advance);
  late final _chat = ChatShown(widget.feed);

  @override
  void initState() {
    super.initState();
    _ticker.start();
  }

  void _advance(Duration elapsed) {
    final index = _frame.value + 1;
    _chat.advance();
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
    _chat.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        RepaintBoundary(child: WallClock(_frame)),
        Expanded(
          child: LayoutBuilder(
            builder: (context, constraints) {
              final width = ((constraints.maxWidth - chatWidth) / patchColumns).floorToDouble();
              final height = (constraints.maxHeight / patchRows).floorToDouble();
              final grid = _Grid(PatchTiles.fill(widget.tier, width, height), width, height);
              return Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Column(
                    children: [
                      for (var row = 0; row < patchRows; row++)
                        Row(
                          children: [
                            for (var column = 0; column < patchColumns; column++)
                              _Patch(Patch.values[row * patchColumns + column], _frame, grid),
                          ],
                        ),
                    ],
                  ),
                  Chat(_frame, _chat, widget.assets),
                ],
              );
            },
          ),
        ),
      ],
    );
  }
}

/// One patch's own box, which cuts what lies past it.
class _Patch extends StatelessWidget {
  final Patch patch;
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Patch(this.patch, this.frame, this.grid);

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    return RepaintBoundary(
      child: SizedBox(
        width: grid.width,
        height: grid.height,
        child: ClipRect(
          child: switch (patch) {
            Patch.still => WallTiles(tiles.columns, tiles.rows, grid.tier),
            Patch.charts => _Charts(grid),
            Patch.recolor => _Recolor(frame, grid),
            Patch.cards => _CardStack(frame, grid),
            Patch.reorder => _Reorder(frame, grid),
            Patch.deep => _Deep(frame, grid),
            Patch.scroll => _ScrollList(frame, grid),
            Patch.paragraph => _Paragraph(frame, grid),
            Patch.pile => _Pile(frame, grid),
          },
        ),
      ),
    );
  }
}

/// Line charts that never change: one painter that never paints again, in the
/// patch's boundary, which keeps its picture.
class _Charts extends StatelessWidget {
  final _Grid grid;

  const _Charts(this.grid);

  @override
  Widget build(BuildContext context) {
    return CustomPaint(size: Size(grid.width, grid.height), painter: _ChartsPainter(grid.tiles));
  }
}

class _ChartsPainter extends CustomPainter {
  final PatchTiles tiles;

  const _ChartsPainter(this.tiles);

  @override
  void paint(Canvas canvas, Size size) {
    final (columns, rows) = patchworkPairs(tiles.columns, tiles.rows);
    final width = patchworkDouble(tiles.width);
    final height = patchworkDouble(tiles.height);
    final step = width / (patchworkChartPoints - 1);
    final stroke = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = patchworkChartStroke;
    for (var index = 0; index < columns * rows; index++) {
      final left = wallGap + (index % columns) * (width + wallGap);
      final top = wallGap + (index ~/ columns) * (height + wallGap);
      final line = Path();
      for (var point = 0; point < patchworkChartPoints; point++) {
        final x = left + point * step;
        final y = top + height * (1 - patchworkChartValue(index, point));
        if (point == 0) {
          line.moveTo(x, y);
        } else {
          line.lineTo(x, y);
        }
      }
      canvas.drawPath(line, stroke..color = Color(patchworkChartArgb(index)));
    }
  }

  @override
  bool shouldRepaint(_ChartsPainter oldDelegate) => false;
}

/// Tiles whose fill changes in turn. A row is two pictures: the tiles' text,
/// built once and kept, and the fills under it, which build again when the
/// row's own step changes.
class _Recolor extends StatelessWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Recolor(this.frame, this.grid);

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    final turn = tiles.inTurn;
    return WallCells([
      for (var row = 0; row < tiles.rows; row++)
        FrameSelect<int>(
          frame,
          (frame) => turn.step(row * tiles.columns, frame),
          (context, step) => Stack(
            children: [
              RepaintBoundary(
                child: ValueListenableBuilder<int>(
                  valueListenable: step,
                  builder: (context, step, _) => Row(
                    spacing: wallGap,
                    children: [
                      for (var column = 0; column < tiles.columns; column++)
                        Container(
                          width: tiles.width,
                          height: tiles.height,
                          decoration: BoxDecoration(
                            color: Color(patchworkRecolorArgb(row * tiles.columns + column, frame.value, turn)),
                            borderRadius: BorderRadius.circular(wallCorner),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
              RepaintBoundary(
                child: Row(
                  spacing: wallGap,
                  children: [
                    for (var column = 0; column < tiles.columns; column++)
                      SizedBox(
                        width: tiles.width,
                        height: tiles.height,
                        child: WallTileBody(row * tiles.columns + column, grid.tier),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
    ]);
  }
}

/// Cards that move every frame. The stack builds again when a move begins;
/// within a move each card's transform listens to the frame for its place.
class _CardStack extends StatelessWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _CardStack(this.frame, this.grid);

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    final cards = Cards.fill(tiles.columns, tiles.rows, tiles.height, grid.height);
    return FrameSelect<int>(
      frame,
      (frame) => frame ~/ patchworkCardFrames,
      (context, moves) => ValueListenableBuilder<int>(
        valueListenable: moves,
        builder: (context, moves, _) {
          final begun = moves * patchworkCardFrames;
          return Stack(
            children: [
              for (var place = 0; place < cards.places; place++)
                if (cards.place(place, begun) case final card?)
                  _Card(card.id, frame, cards, grid, key: ValueKey(card.id)),
            ],
          );
        },
      ),
    );
  }
}

/// A card: its tiles are a picture of their own, which the transform over it
/// moves without painting it again. A card no frame shows waits above the
/// patch.
class _Card extends StatelessWidget {
  final int id;
  final ValueListenable<int> frame;
  final Cards cards;
  final _Grid grid;

  const _Card(this.id, this.frame, this.cards, this.grid, {super.key});

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    return ValueListenableBuilder<int>(
      valueListenable: frame,
      child: RepaintBoundary(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: wallGap),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            spacing: wallGap,
            children: [
              Container(
                width: patchworkReorderWidth(cards.columns, tiles.width),
                height: tiles.height,
                padding: EdgeInsets.all(tiles.height * 0.06),
                decoration: BoxDecoration(
                  color: Color(patchworkCardArgb(id)),
                  borderRadius: BorderRadius.circular(wallCorner),
                ),
                child: Text(
                  patchworkCardTitle(id),
                  style: inkText(tiles.height * 0.30, bold: true),
                  softWrap: false,
                  overflow: TextOverflow.clip,
                  maxLines: 1,
                ),
              ),
              for (var row = 0; row < cards.rows; row++)
                Row(
                  spacing: wallGap,
                  children: [
                    for (var column = 0; column < cards.columns; column++)
                      WallTile(patchworkCardTile(id, row * cards.columns + column, cards.tiles), grid.tier),
                  ],
                ),
            ],
          ),
        ),
      ),
      builder: (context, frame, card) =>
          Transform.translate(offset: Offset(0, cards.top(id, frame) ?? -cards.height), child: card),
    );
  }
}

/// A wrapping row of items, two of which change places every frame: every
/// item is a widget built once, a boundary of its own and under its key, so
/// a frame moves those two and builds none again.
class _Reorder extends StatefulWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Reorder(this.frame, this.grid);

  @override
  State<_Reorder> createState() => _ReorderState();
}

class _ReorderState extends State<_Reorder> {
  late final _count = patchworkReorderCount(widget.grid.tiles.count);
  late final _items = [
    for (var item = 0; item < _count; item++)
      RepaintBoundary(
        key: ValueKey(item),
        child: WallTile(
          item,
          widget.grid.tier,
          width: patchworkReorderWidth(patchworkReorderUnits(item), widget.grid.tiles.width),
        ),
      ),
  ];

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(wallGap),
      child: ValueListenableBuilder<int>(
        valueListenable: widget.frame,
        builder: (context, frame, _) => Wrap(
          spacing: wallGap,
          runSpacing: wallGap,
          children: [
            for (var position = 0; position < _count; position++) _items[patchworkReorderItem(position, frame, _count)],
          ],
        ),
      ),
    );
  }
}

/// Nests of boxes around digits that change in turn. A row of nests is two
/// pictures: its boxes, built once and kept, and its digits over them, which
/// build again when the row's own step changes.
class _Deep extends StatelessWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Deep(this.frame, this.grid);

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    final (columns, rows) = patchworkPairs(tiles.columns, tiles.rows);
    final turn = InTurn(columns, rows);
    final padding = tiles.height * 0.06;
    return WallCells([
      for (var row = 0; row < rows; row++)
        FrameSelect<int>(
          frame,
          (frame) => turn.step(row * columns, frame),
          (context, step) => Stack(
            children: [
              RepaintBoundary(
                child: Row(
                  spacing: wallGap,
                  children: [
                    for (var column = 0; column < columns; column++) _Nest(row * columns + column, padding, tiles),
                  ],
                ),
              ),
              RepaintBoundary(
                child: ValueListenableBuilder<int>(
                  valueListenable: step,
                  builder: (context, step, _) => Row(
                    spacing: wallGap,
                    children: [
                      for (var column = 0; column < columns; column++)
                        SizedBox(
                          width: patchworkDouble(tiles.width),
                          height: patchworkDouble(tiles.height),
                          child: Padding(
                            padding: EdgeInsets.all(padding * patchworkDepth),
                            child: Text(
                              patchworkDeepNumber(row * columns + column, frame.value, turn).toString().padLeft(3, '0'),
                              style: inkText(tiles.height * 0.30, bold: true),
                              softWrap: false,
                              overflow: TextOverflow.clip,
                              maxLines: 1,
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
    ]);
  }
}

/// The boxes of nest [index], each inside the one around it.
class _Nest extends StatelessWidget {
  final int index;
  final double padding;
  final PatchTiles tiles;

  const _Nest(this.index, this.padding, this.tiles);

  @override
  Widget build(BuildContext context) {
    Widget nest = ColoredBox(color: Color(patchworkDeepArgb(index, patchworkDepth - 1)));
    for (var level = patchworkDepth - 2; level >= 0; level--) {
      nest = ColoredBox(
        color: Color(patchworkDeepArgb(index, level)),
        child: Padding(padding: EdgeInsets.all(padding), child: nest),
      );
    }
    return SizedBox(width: patchworkDouble(tiles.width), height: patchworkDouble(tiles.height), child: nest);
  }
}

/// A lazy list of rows of tiles, scrolled by one row every frame. Each row
/// is the list's own repaint boundary, built when it enters.
class _ScrollList extends StatefulWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _ScrollList(this.frame, this.grid);

  @override
  State<_ScrollList> createState() => _ScrollListState();
}

class _ScrollListState extends State<_ScrollList> {
  final _controller = ScrollController();
  late final _total = patchworkScrollRows(widget.grid.tiles.columns);

  @override
  void initState() {
    super.initState();
    widget.frame.addListener(_scroll);
  }

  void _scroll() {
    // The first frame ticks before the list has a position.
    if (!_controller.hasClients) return;
    final tiles = widget.grid.tiles;
    final first = patchworkScrollFirst(widget.frame.value, _total, tiles.rows);
    _controller.jumpTo(first * (tiles.height + wallGap));
  }

  @override
  void dispose() {
    widget.frame.removeListener(_scroll);
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tiles = widget.grid.tiles;
    return ListView.builder(
      controller: _controller,
      physics: const NeverScrollableScrollPhysics(),
      padding: const EdgeInsets.only(top: wallGap),
      itemExtent: tiles.height + wallGap,
      itemCount: _total,
      addAutomaticKeepAlives: false,
      itemBuilder: (context, row) => Padding(
        padding: const EdgeInsets.only(left: wallGap, bottom: wallGap),
        child: Row(
          spacing: wallGap,
          children: [
            for (var column = 0; column < tiles.columns; column++)
              WallTile(row * tiles.columns + column, widget.grid.tier),
          ],
        ),
      ),
    );
  }
}

/// One paragraph, one word of which changes every frame: Flutter lays the
/// paragraph out whole when its text changes.
class _Paragraph extends StatelessWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Paragraph(this.frame, this.grid);

  @override
  Widget build(BuildContext context) {
    final tiles = grid.tiles;
    final words = patchworkParagraphWords(tiles.count);
    final style = inkText(tiles.height * 0.30);
    return Padding(
      padding: const EdgeInsets.all(wallGap),
      child: ValueListenableBuilder<int>(
        valueListenable: frame,
        builder: (context, frame, _) => Text(patchworkParagraph(frame, words), style: style),
      ),
    );
  }
}

/// Translucent tiles lying over one another, each a boundary of its own. The
/// stack builds again for the two tiles that change: the one that turns
/// opaque and the one that turns translucent.
class _Pile extends StatefulWidget {
  final ValueListenable<int> frame;
  final _Grid grid;

  const _Pile(this.frame, this.grid);

  @override
  State<_Pile> createState() => _PileState();
}

class _PileState extends State<_Pile> {
  late final (int, int) _pile = patchworkPileGrid(widget.grid.tiles.columns, widget.grid.tiles.rows);
  late final _tiles = [for (var index = 0; index < _pile.$1 * _pile.$2; index++) _tile(index, translucent: true)];

  Widget _tile(int index, {required bool translucent}) {
    final tiles = widget.grid.tiles;
    final fill = translucent
        ? Color(wallFillArgb(index)).withAlpha(patchworkPileAlpha)
        : const Color(patchworkPileOpaqueArgb);
    return Positioned(
      left: wallGap + (index % _pile.$1) * (tiles.width + wallGap),
      top: wallGap + (index ~/ _pile.$1) * (tiles.height + wallGap),
      child: RepaintBoundary(
        child: WallTile(
          index,
          widget.grid.tier,
          width: patchworkDouble(tiles.width),
          height: patchworkDouble(tiles.height),
          fill: fill,
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final count = _tiles.length;
    return FrameSelect<int>(
      widget.frame,
      (frame) => patchworkPileOpaque(frame, count),
      (context, opaque) => ValueListenableBuilder<int>(
        valueListenable: opaque,
        builder: (context, opaque, _) => Stack(
          children: [
            for (var index = 0; index < count; index++)
              index == opaque ? _tile(index, translucent: false) : _tiles[index],
          ],
        ),
      ),
    );
  }
}
