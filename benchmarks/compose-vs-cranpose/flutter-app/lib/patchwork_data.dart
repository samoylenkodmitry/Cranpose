// The patchwork's data, as `perf-data/src/patchwork.rs` makes it bit for bit:
// nine patches of the still wall's tiles, three across and three down. Two
// never change; in the other seven a frame changes a small part and leaves
// the rest as it was. Dart has no 32-bit floats: the chart heights and the
// cards' tops are computed in 64 bits.

import 'dart:math' as math;

import 'data.dart';
import 'wall.dart';

const patchColumns = 3;
const patchRows = 3;

/// The patches in the order the screen shows them, row by row.
enum Patch { still, charts, recolor, cards, reorder, deep, scroll, paragraph, pile }

/// The step, in tiles or words, of everything that jumps.
const _jump = 37;

/// One patch's tiles: [columns] by [rows] of them, of the tier's size.
class PatchTiles {
  final int columns;
  final int rows;
  final WallTier tier;

  const PatchTiles(this.columns, this.rows, this.tier);

  /// The tiles of [tier] that fill a patch [patchWidth] by [patchHeight] points.
  factory PatchTiles.fill(WallTier tier, double patchWidth, double patchHeight) {
    final (columns, rows) = wallGrid(tier, patchWidth, patchHeight);
    return PatchTiles(columns, rows, tier);
  }

  double get width => tier.tileWidth;
  double get height => tier.tileHeight;
  int get count => columns * rows;
  InTurn get inTurn => InTurn(columns, rows);
}

/// The items of a patch that change in turn: [columns] by [rows] of them.
/// Each frame changes the items of one row, the next row down the frame after.
class InTurn {
  final int columns;
  final int rows;

  const InTurn(this.columns, this.rows);

  /// The changes of item [index] up to frame [frame].
  int step(int index, int frame) => (index ~/ math.max(columns, 1) + frame) ~/ math.max(rows, 1);
}

/// The points of one chart of the charts patch, and the width of its line.
const patchworkChartPoints = 512;
const patchworkChartStroke = 1.0;

/// The height of chart [index] at [point], as a fraction of the chart.
double patchworkChartValue(int index, int point) {
  final at = point * 0.125;
  return 0.5 + 0.38 * math.sin(at * 0.32 + index * 0.77) + 0.08 * math.sin(at * 1.7 + index);
}

/// The color of chart [index]'s line.
int patchworkChartArgb(int index) => paletteArgb[index % paletteArgb.length];

/// The fill of tile [index] of the recolor patch in frame [frame].
int patchworkRecolorArgb(int index, int frame, InTurn turn) =>
    paletteArgb[(index + turn.step(index, frame)) % paletteArgb.length];

/// The frames one move of the cards patch takes.
const patchworkCardFrames = 32;

/// A card of the cards patch in a frame: its number and its top in points
/// from the patch's top.
class CardPlace {
  final int id;
  final double top;

  const CardPlace(this.id, this.top);
}

/// The cards patch: a card is [columns] tiles across and [rows] rows of
/// tiles under a header, [height] points tall, and [slots] cards rest in the
/// patch.
class Cards {
  final int columns;
  final int rows;
  final double height;
  final int slots;

  const Cards(this.columns, this.rows, this.height, this.slots);

  /// The cards of a patch [columns] by [rows] tiles of [tileHeight] points,
  /// [patchHeight] points tall.
  factory Cards.fill(int columns, int rows, double tileHeight, double patchHeight) {
    final cardRows = math.max(rows ~/ 4, 1);
    final height = tileHeight + cardRows * (tileHeight + wallGap);
    return Cards(columns, cardRows, height, math.max(((patchHeight - wallGap) / (height + wallGap)).toInt(), 1));
  }

  /// The places a frame may show a card at.
  int get places => slots + 1;

  int get tiles => columns * rows;

  /// The card at place [place] in frame [frame], the topmost first; none at
  /// the last place while no card is on its way in. The moves alternate: a
  /// new card slides in and the others move down by a card, then the first
  /// and the third card move to each other's places and back.
  CardPlace? place(int place, int frame) {
    final moves = frame ~/ patchworkCardFrames;
    final along = (frame % patchworkCardFrames) / patchworkCardFrames;
    final pitch = height + wallGap;
    final came = (moves + 1) ~/ 2 + (moves + 1) % 2;
    if (moves % 2 == 0) {
      final id = came + slots - 1 - place;
      return id < 0 ? null : CardPlace(id, wallGap + (place - 1 + along) * pitch);
    }
    if (place >= slots) return null;
    final other = math.min(2, slots - 1);
    final reach = other * pitch * (1 - (2 * along - 1).abs());
    final moved = place == 0
        ? reach
        : place == other
        ? -reach
        : 0.0;
    return CardPlace(came + slots - 1 - place, wallGap + place * pitch + moved);
  }

  /// The top of card [id] in frame [frame], while the frame shows it.
  double? top(int id, int frame) {
    for (var place = 0; place < places; place++) {
      final card = this.place(place, frame);
      if (card != null && card.id == id) return card.top;
    }
    return null;
  }
}

/// The header of card [id].
String patchworkCardTitle(int id) => 'Group ${(id % 100000).toString().padLeft(5, '0')}';

/// The fill of card [id]'s header.
int patchworkCardArgb(int id) => gradientEndArgb[id % gradientEndArgb.length];

/// The wall tile at place [tile] of card [id], whose card holds [tiles] tiles.
int patchworkCardTile(int id, int tile, int tiles) => (id * tiles + tile) % 100000;

/// The items of the reorder patch for a patch of [tiles] tiles.
int patchworkReorderCount(int tiles) => math.max(tiles * 2 ~/ 5, 1);

/// The first of the two neighbouring places whose items change places in frame [frame].
int patchworkReorderSwap(int frame, int count) => (frame * _jump) % math.max(count - 1, 1);

/// The item at place [position] of the reorder patch in frame [frame].
int patchworkReorderItem(int position, int frame, int count) {
  final swap = patchworkReorderSwap(frame, count);
  if (count < 2) return position;
  if (position == swap) return position + 1;
  if (position == swap + 1) return position - 1;
  return position;
}

/// The width of item [item] of the reorder patch, in tiles: 1 to 3.
int patchworkReorderUnits(int item) => (((item * 2654435761) & 0xFFFFFFFF) >> 22) % 3 + 1;

/// The width in points of an item [units] tiles wide.
double patchworkReorderWidth(int units, double tileWidth) => units * tileWidth + (units - 1) * wallGap;

/// The boxes around the digits of one nest of the deep patch.
const patchworkDepth = 12;

/// The nests or the charts across and down a patch of [columns] by [rows] tiles.
(int, int) patchworkPairs(int columns, int rows) => (math.max(columns ~/ 2, 1), math.max(rows ~/ 2, 1));

/// The width or height of an item that covers 2 tiles of [tile] points and the gap between them.
double patchworkDouble(double tile) => 2 * tile + wallGap;

/// The fill of box [level] of nest [index], the outermost box being level 0.
int patchworkDeepArgb(int index, int level) => paletteArgb[(index + level) % paletteArgb.length];

/// The number inside nest [index] in frame [frame], under 1000.
int patchworkDeepNumber(int index, int frame, InTurn turn) => (index * 7 + turn.step(index, frame)) % 1000;

/// The rows of the scroll patch's list for rows of [columns] tiles.
int patchworkScrollRows(int columns) => 100000 ~/ math.max(columns, 1);

/// The first row the scroll patch shows in frame [frame].
int patchworkScrollFirst(int frame, int total, int visible) => frame % (math.max(total - visible, 0) + 1);

/// The words of the paragraph patch for a patch of [tiles] tiles.
int patchworkParagraphWords(int tiles) => tiles * 3;

/// The paragraph in frame [frame]: [words] five-digit words, one of which shows the frame.
String patchworkParagraph(int frame, int words) {
  final changed = (frame * _jump) % math.max(words, 1);
  final text = StringBuffer();
  for (var word = 0; word < words; word++) {
    if (word > 0) text.write(' ');
    text.write(wallLabel(word == changed ? frame % 100000 : word));
  }
  return text.toString();
}

/// The tiles across and down the pile patch for a patch of [columns] by [rows] tiles.
(int, int) patchworkPileGrid(int columns, int rows) => (math.max(columns - 1, 1), math.max(rows - 1, 1));

/// The opacity of the pile patch's tiles' fills, of 255, and the fill of its opaque tile.
const patchworkPileAlpha = 0x99;
const patchworkPileOpaqueArgb = 0xFF000000;

/// The tile of the pile patch whose fill is opaque in frame [frame].
int patchworkPileOpaque(int frame, int tiles) => (frame * _jump) % math.max(tiles, 1);
