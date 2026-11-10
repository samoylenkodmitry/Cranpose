// The patchwork's data, as `perf-data/src/patchwork.rs` makes it bit for bit:
// nine patches of the still wall's tiles, three across and three down. Two
// never change; in the other seven a frame changes a small part and leaves
// the rest as it was. The Avalonia app compiles this file.

namespace PerfCompare;

/// <summary>The patches in the order the screen shows them, row by row.</summary>
public enum Patch { Still, Charts, Recolor, Cards, Reorder, Deep, Scroll, Paragraph, Pile }

/// <summary>The items of a patch that change in turn: <c>Columns</c> by <c>Rows</c> tiles, cells or
/// nests. Each frame changes the items of one row, the next row down the frame after.</summary>
public readonly record struct InTurn(int Columns, int Rows)
{
    /// <summary>The changes of item <c>index</c> up to frame <c>frame</c>.</summary>
    public int Step(int index, int frame) => (index / Math.Max(Columns, 1) + frame) / Math.Max(Rows, 1);

    /// <summary>The row whose items change in frame <c>frame</c>, for frames from 1 on.</summary>
    public int ChangedRow(int frame) => (Rows - frame % Rows) % Rows;
}

/// <summary>One patch's tiles: <c>Columns</c> by <c>Rows</c> of them, <c>Width</c> by <c>Height</c> points each.</summary>
public readonly record struct PatchTiles(int Columns, int Rows, float Width, float Height)
{
    public int Count => Columns * Rows;

    public InTurn InTurn => new(Columns, Rows);
}

/// <summary>A card of the cards patch in a frame: its number and its top in points from the patch's top.</summary>
public readonly record struct CardPlace(int Id, float Top);

/// <summary>The cards patch: a card is <c>Columns</c> tiles across and <c>Rows</c> rows of tiles under a
/// header, <c>Height</c> points tall, and <c>Slots</c> cards rest in the patch.</summary>
public readonly record struct Cards(int Columns, int Rows, float Height, int Slots)
{
    /// <summary>The places a frame may show a card at.</summary>
    public int Places => Slots + 1;

    public int Tiles => Columns * Rows;

    /// <summary>The cards of a patch <c>columns</c> by <c>rows</c> tiles of <c>tileHeight</c> points, in a patch
    /// <c>patchHeight</c> points tall: a card holds a quarter of the patch's rows.</summary>
    public static Cards Of(int columns, int rows, float tileHeight, float patchHeight)
    {
        var cardRows = Math.Max(rows / 4, 1);
        var height = tileHeight + cardRows * (tileHeight + PatchworkData.Gap);
        var slots = Math.Max((int)((patchHeight - PatchworkData.Gap) / (height + PatchworkData.Gap)), 1);
        return new Cards(columns, cardRows, height, slots);
    }

    /// <summary>The card at place <c>place</c> in frame <c>frame</c>, the topmost first; none at the last place
    /// while no card is on its way in. The moves alternate, <see cref="PatchworkData.CardFrames"/> frames each:
    /// a new card slides in and the others move down by a card, then the first and the third card move to
    /// each other's places and back.</summary>
    public CardPlace? Place(int place, int frame)
    {
        var moves = frame / PatchworkData.CardFrames;
        var along = frame % PatchworkData.CardFrames / (float)PatchworkData.CardFrames;
        var pitch = Height + PatchworkData.Gap;
        var came = (moves + 1) / 2 + (moves + 1) % 2;
        if (moves % 2 == 0)
        {
            var id = came + Slots - 1 - place;
            return id < 0 ? null : new CardPlace(id, PatchworkData.Gap + (place - 1f + along) * pitch);
        }
        if (place >= Slots) return null;
        var other = Math.Min(2, Slots - 1);
        var reach = other * pitch * (1f - MathF.Abs(2f * along - 1f));
        var moved = place == 0 ? reach : place == other ? -reach : 0f;
        return new CardPlace(came + Slots - 1 - place, PatchworkData.Gap + place * pitch + moved);
    }
}

public static class PatchworkData
{
    /// <summary>The space between two tiles and around a patch's tiles, in points.</summary>
    public const float Gap = 1;

    /// <summary>The patches across and down the screen.</summary>
    public const int PatchColumns = 3;
    public const int PatchRows = 3;

    /// <summary>The step, in tiles or words, of everything that jumps.</summary>
    const int Jump = 37;

    /// <summary>The points of one chart of the charts patch, and the width of its line.</summary>
    public const int ChartPoints = 512;
    public const float ChartStroke = 1f;

    /// <summary>The frames one move of the cards patch takes.</summary>
    public const int CardFrames = 32;

    /// <summary>The boxes around the digits of one nest of the deep patch.</summary>
    public const int Depth = 12;

    /// <summary>The opacity of the pile patch's tiles' fills, of 255, and the fill of its opaque tile.</summary>
    public const byte PileAlpha = 0x99;
    public const uint PileOpaqueArgb = 0xFF000000;

    /// <summary>The tiles of <c>tileWidth</c> by <c>tileHeight</c> points that fill a patch
    /// <c>patchWidth</c> by <c>patchHeight</c> points.</summary>
    public static PatchTiles Tiles(float tileWidth, float tileHeight, float patchWidth, float patchHeight)
    {
        static int Fit(float space, float tile) => Math.Max(1, (int)MathF.Floor((space - Gap) / (tile + Gap)));
        return new PatchTiles(Fit(patchWidth, tileWidth), Fit(patchHeight, tileHeight), tileWidth, tileHeight);
    }

    /// <summary>The height of chart <c>index</c> at <c>point</c>, as a fraction of the chart.</summary>
    public static float ChartValue(int index, int point)
    {
        var at = point * 0.125f;
        return 0.5f + 0.38f * MathF.Sin(at * 0.32f + index * 0.77f) + 0.08f * MathF.Sin(at * 1.7f + index);
    }

    /// <summary>The palette index of chart <c>index</c>'s line.</summary>
    public static int ChartColor(int index) => index % PerfData.PaletteArgb.Length;

    /// <summary>The palette index of tile <c>index</c> of the recolor patch in frame <c>frame</c>.</summary>
    public static int RecolorColor(int index, int frame, InTurn turn) =>
        (index + turn.Step(index, frame)) % PerfData.PaletteArgb.Length;

    /// <summary>The header of card <c>id</c>.</summary>
    public static string CardTitle(int id) => $"Group {id % 100_000:D5}";

    /// <summary>The wall tile at place <c>tile</c> of card <c>id</c>, whose card holds <c>tiles</c> tiles.</summary>
    public static int CardTile(int id, int tile, int tiles) => (int)(((long)id * tiles + tile) % 100_000);

    /// <summary>The items of the reorder patch for a patch of <c>tiles</c> tiles.</summary>
    public static int ReorderCount(int tiles) => Math.Max(tiles * 2 / 5, 1);

    /// <summary>The first of the two neighbouring places whose items change places in frame <c>frame</c>.</summary>
    public static int ReorderSwap(int frame, int count) => (int)((long)frame * Jump % Math.Max(count - 1, 1));

    /// <summary>The item at place <c>position</c> of the reorder patch in frame <c>frame</c>.</summary>
    public static int ReorderItem(int position, int frame, int count)
    {
        var swap = ReorderSwap(frame, count);
        if (count < 2) return position;
        if (position == swap) return position + 1;
        if (position == swap + 1) return position - 1;
        return position;
    }

    /// <summary>The width of item <c>item</c> of the reorder patch, in tiles: 1 to 3.</summary>
    public static int ReorderUnits(int item) => (int)((unchecked((uint)item * 2_654_435_761u) >> 22) % 3) + 1;

    /// <summary>The width in points of an item <c>units</c> tiles wide.</summary>
    public static float ReorderWidth(int units, float tileWidth) => units * tileWidth + (units - 1) * Gap;

    /// <summary>The nests or the charts across and down a patch of <c>columns</c> by <c>rows</c> tiles.</summary>
    public static (int Columns, int Rows) Pairs(int columns, int rows) => (Math.Max(columns / 2, 1), Math.Max(rows / 2, 1));

    /// <summary>The width or height of an item that covers 2 tiles of <c>tile</c> points and the gap between them.</summary>
    public static float Double(float tile) => 2f * tile + Gap;

    /// <summary>The palette index of box <c>level</c> of nest <c>index</c>, the outermost box being level 0.</summary>
    public static int DeepColor(int index, int level) => (index + level) % PerfData.PaletteArgb.Length;

    /// <summary>The number inside nest <c>index</c> in frame <c>frame</c>, under 1000.</summary>
    public static int DeepNumber(int index, int frame, InTurn turn) => (index * 7 + turn.Step(index, frame)) % 1000;

    /// <summary>The rows of the scroll patch's list for rows of <c>columns</c> tiles.</summary>
    public static int ScrollRows(int columns) => 100_000 / Math.Max(columns, 1);

    /// <summary>The first row the scroll patch shows in frame <c>frame</c>.</summary>
    public static int ScrollFirst(int frame, int total, int visible) => frame % (Math.Max(total - visible, 0) + 1);

    /// <summary>The words of the paragraph patch for a patch of <c>tiles</c> tiles.</summary>
    public static int ParagraphWords(int tiles) => tiles * 3;

    /// <summary>The word of the paragraph that shows the frame in frame <c>frame</c>.</summary>
    public static int ParagraphChanged(int frame, int words) => (int)((long)frame * Jump % Math.Max(words, 1));

    /// <summary>The paragraph in frame <c>frame</c>: <c>words</c> five-digit words, one of which shows the frame.</summary>
    public static string Paragraph(int frame, int words)
    {
        var changed = ParagraphChanged(frame, words);
        var text = new System.Text.StringBuilder(words * 6);
        for (var word = 0; word < words; word++)
        {
            if (word > 0) text.Append(' ');
            text.Append(Label(word == changed ? frame % 100_000 : word));
        }
        return text.ToString();
    }

    /// <summary>The tiles across and down the pile patch for a patch of <c>columns</c> by <c>rows</c> tiles.</summary>
    public static (int Columns, int Rows) PileGrid(int columns, int rows) => (Math.Max(columns - 1, 1), Math.Max(rows - 1, 1));

    /// <summary>The tile of the pile patch whose fill is opaque in frame <c>frame</c>.</summary>
    public static int PileOpaque(int frame, int tiles) => (int)((long)frame * Jump % Math.Max(tiles, 1));

    /// <summary>A tile's label: its index in five digits.</summary>
    public static string Label(int index) => index.ToString("D5");
}
