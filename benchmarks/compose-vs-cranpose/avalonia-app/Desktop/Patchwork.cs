// The patchwork in Avalonia (`../../README.md`): nine patches of the still
// wall's tiles beside a chat. Two patches never change; in the others a
// frame changes a small part. Its data follows `perf-data/src/patchwork.rs`
// bit for bit (`../../shared-cs/PatchworkData.cs`).
//
// In Avalonia's idiom every patch builds its controls once and a frame sets
// only the property that changed: the brushes of the changed row of tiles,
// the text of the changed row of nests, the translation of each card (a
// render transform, which lays out nothing), the order of two items in a
// WrapPanel, one row of a recycling list, one word of a paragraph, the
// brushes of two tiles of the pile. The charts are one control that builds
// its geometries once and draws them from the retained render commands.

using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Immutable;
using PerfCompare;
using static PerfCompare.PatchworkData;

namespace PerfAvalonia;

/// <summary>One patch: its tiles and its size in points.</summary>
sealed record PatchGrid(Look Look, PatchTiles Tiles, double Width, double Height)
{
    public double TileWidth => Tiles.Width;

    public double TileHeight => Tiles.Height;

    /// <summary>Where item `index` lies on a grid of `columns` items `width` by `height` points.</summary>
    public static Point Cell(int index, int columns, double width, double height) =>
        new(Gap + index % columns * (width + Gap), Gap + index / columns * (height + Gap));
}

public sealed class PatchworkView : Grid
{
    /// <summary>The window every desktop app opens, under the top bar and the clock line.</summary>
    const double AreaWidth = 1280;
    const double AreaHeight = 820 - 56 - WallData.ClockHeight;

    readonly int freeze;
    readonly TextBlock clock;
    bool firstFrameLogged;

    public PatchworkView(int tierIndex, int freeze)
    {
        this.freeze = freeze;
        var tier = WallData.Tier(tierIndex);
        var patchWidth = MathF.Floor((float)(AreaWidth - ChatData.Width) / PatchColumns);
        var patchHeight = MathF.Floor((float)AreaHeight / PatchRows);
        var look = new Look(1);
        var grid = new PatchGrid(
            look, Tiles((float)tier.Width, (float)tier.Height, patchWidth, patchHeight), patchWidth, patchHeight);
        TextElement.SetFontFamily(this, new FontFamily(DeviceRoboto.Family));
        Background = new ImmutableSolidColorBrush(Color.FromUInt32(WallData.BackgroundArgb));
        RowDefinitions = new RowDefinitions($"56,{WallData.ClockHeight},*");
        Children.Add(Look.TopBar("Patchwork"));

        clock = look.Text(WallData.ClockText, Brushes.White);
        clock.Margin = new Thickness(8, 0);
        clock.VerticalAlignment = VerticalAlignment.Center;
        SetRow(clock, 1);
        Children.Add(clock);

        var patches = new Canvas();
        for (var index = 0; index < PatchColumns * PatchRows; index++)
        {
            var patch = Build((Patch)index, grid);
            Canvas.SetLeft(patch, index % PatchColumns * patchWidth);
            Canvas.SetTop(patch, index / PatchColumns * patchHeight);
            patches.Children.Add(patch);
        }
        var body = new Grid { ColumnDefinitions = new ColumnDefinitions($"*,{ChatData.Width}") };
        body.Children.Add(patches);
        var chat = new ChatView();
        SetColumn(chat, 1);
        body.Children.Add(chat);
        SetRow(body, 2);
        Children.Add(body);
    }

    static Control Build(Patch patch, PatchGrid grid) => patch switch
    {
        Patch.Still => new TileCanvas(grid, index => Look.Palette[index % Look.Palette.Length]),
        Patch.Charts => new ChartsPatch(grid),
        Patch.Recolor => new RecolorPatch(grid),
        Patch.Cards => new CardsPatch(grid),
        Patch.Reorder => new ReorderPatch(grid),
        Patch.Deep => new DeepPatch(grid),
        Patch.Scroll => new ScrollPatch(grid),
        Patch.Paragraph => new ParagraphPatch(grid),
        _ => new PilePatch(grid),
    };

    protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e)
    {
        base.OnAttachedToVisualTree(e);
        TopLevel.GetTopLevel(this)?.RequestAnimationFrame(OnFrame);
    }

    void OnFrame(TimeSpan _)
    {
        if (!firstFrameLogged)
        {
            firstFrameLogged = true;
            Launch.Log("PERF first_frame");
        }
        var frame = Clock.Advance();
        clock.Text = WallData.Clock(frame);
        if (freeze > 0 && frame >= freeze)
        {
            Launch.Log($"PERF frozen frame={frame}");
            return;
        }
        TopLevel.GetTopLevel(this)?.RequestAnimationFrame(OnFrame);
    }
}

/// <summary>A patch's box, which cuts what lies past it.</summary>
abstract class PatchBox : Canvas
{
    protected PatchBox(PatchGrid grid)
    {
        Width = grid.Width;
        Height = grid.Height;
        ClipToBounds = true;
    }

    /// <summary>Calls `onFrame` every frame while the patch is on screen.</summary>
    protected void Follow(Action<int> onFrame) => Clock.Follow(this, onFrame);
}

/// <summary>Tiles on the grid of the still wall, each on the fill `fill` gives it.</summary>
class TileCanvas : PatchBox
{
    protected readonly TileView[] Tiles;

    public TileCanvas(PatchGrid grid, Func<int, IBrush> fill) : base(grid)
    {
        Tiles = new TileView[grid.Tiles.Count];
        for (var index = 0; index < Tiles.Length; index++)
        {
            var tile = new TileView(grid.Look, grid.TileWidth, grid.TileHeight, grid.TileHeight, grid.TileHeight * 0.06);
            tile.Bind(index);
            tile.Background = fill(index);
            var at = PatchGrid.Cell(index, grid.Tiles.Columns, grid.TileWidth, grid.TileHeight);
            SetLeft(tile, at.X);
            SetTop(tile, at.Y);
            Children.Add(tile);
            Tiles[index] = tile;
        }
    }
}

/// <summary>Line charts that never change. One control builds every chart's geometry once; its render
/// commands are retained, so no frame draws them again.</summary>
sealed class ChartsPatch : PatchBox
{
    public ChartsPatch(PatchGrid grid) : base(grid) => Children.Add(new Charts(grid) { Width = grid.Width, Height = grid.Height });

    sealed class Charts : Control
    {
        readonly (StreamGeometry Line, IPen Pen)[] lines;

        public Charts(PatchGrid grid)
        {
            var (columns, rows) = Pairs(grid.Tiles.Columns, grid.Tiles.Rows);
            var width = Double(grid.Tiles.Width);
            var height = Double(grid.Tiles.Height);
            lines = new (StreamGeometry, IPen)[columns * rows];
            for (var index = 0; index < lines.Length; index++)
            {
                var origin = PatchGrid.Cell(index, columns, width, height);
                var step = width / (ChartPoints - 1);
                var line = new StreamGeometry();
                using (var path = line.Open())
                {
                    for (var point = 0; point < ChartPoints; point++)
                    {
                        var at = new Point(origin.X + point * step, origin.Y + height * (1 - ChartValue(index, point)));
                        if (point == 0) path.BeginFigure(at, isFilled: false);
                        else path.LineTo(at);
                    }
                    path.EndFigure(isClosed: false);
                }
                lines[index] = (line, new ImmutablePen(
                    new ImmutableSolidColorBrush(Color.FromUInt32(PerfData.PaletteArgb[ChartColor(index)])), ChartStroke));
            }
        }

        public override void Render(DrawingContext context)
        {
            foreach (var (line, pen) in lines) context.DrawGeometry(null, pen, line);
        }
    }
}

/// <summary>Tiles whose fill changes in turn: a frame sets the brushes of one row.</summary>
sealed class RecolorPatch : TileCanvas
{
    readonly int columns;
    readonly InTurn turn;

    public RecolorPatch(PatchGrid grid)
        : base(grid, index => Look.Palette[RecolorColor(index, 0, grid.Tiles.InTurn)])
    {
        columns = grid.Tiles.Columns;
        turn = grid.Tiles.InTurn;
        Follow(Show);
    }

    void Show(int frame)
    {
        var first = turn.ChangedRow(frame) * columns;
        for (var index = first; index < first + columns; index++)
        {
            Tiles[index].Background = Look.Palette[RecolorColor(index, frame, turn)];
        }
    }
}

/// <summary>A card: a header over rows of tiles. It moves by a render transform and rebinds to another
/// card's number without new controls.</summary>
sealed class PatchCard : StackPanel
{
    readonly Cards cards;
    readonly TextBlock title;
    readonly Border header;
    readonly TileView[] tiles;

    public PatchCard(PatchGrid grid, Cards cards)
    {
        this.cards = cards;
        Spacing = Gap;
        Margin = new Thickness(Gap, 0);
        RenderTransform = Shift;
        title = grid.Look.Text(grid.TileHeight * 0.30, Brushes.White, bold: true);
        header = new Border
        {
            Width = ReorderWidth(cards.Columns, (float)grid.TileWidth),
            Height = grid.TileHeight,
            CornerRadius = new CornerRadius(WallData.Corner),
            Padding = new Thickness(grid.TileHeight * 0.06),
            ClipToBounds = true,
            Child = title,
        };
        Children.Add(header);
        tiles = new TileView[cards.Tiles];
        for (var row = 0; row < cards.Rows; row++)
        {
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = Gap };
            for (var column = 0; column < cards.Columns; column++)
            {
                var tile = new TileView(grid.Look, grid.TileWidth, grid.TileHeight, grid.TileHeight, grid.TileHeight * 0.06);
                tiles[row * cards.Columns + column] = tile;
                line.Children.Add(tile);
            }
            Children.Add(line);
        }
    }

    public TranslateTransform Shift { get; } = new();

    /// <summary>Shows card `id`.</summary>
    public void Bind(int id)
    {
        header.Background = Look.GradientEnd[id % Look.GradientEnd.Length];
        title.Text = CardTitle(id);
        for (var tile = 0; tile < tiles.Length; tile++)
        {
            var index = CardTile(id, tile, cards.Tiles);
            tiles[tile].Bind(index);
            tiles[tile].Background = Look.Palette[index % Look.Palette.Length];
        }
    }
}

/// <summary>Cards that move every frame. The set of cards changes when a move begins, one card in and one
/// out; within a move a frame sets each card's translation and nothing else.</summary>
sealed class CardsPatch : PatchBox
{
    readonly PatchGrid grid;
    readonly Cards cards;
    readonly PatchCard?[] placed;
    readonly Dictionary<int, PatchCard> shown = [];
    readonly Stack<PatchCard> spare = [];
    int move = -1;

    public CardsPatch(PatchGrid grid) : base(grid)
    {
        this.grid = grid;
        cards = Cards.Of(grid.Tiles.Columns, grid.Tiles.Rows, (float)grid.TileHeight, (float)grid.Height);
        placed = new PatchCard?[cards.Places];
        Show(0);
        Follow(Show);
    }

    void Show(int frame)
    {
        var moves = frame / CardFrames;
        if (moves != move)
        {
            move = moves;
            Place(moves * CardFrames);
        }
        for (var place = 0; place < placed.Length; place++)
        {
            if (placed[place] is { } card) card.Shift.Y = cards.Place(place, frame)?.Top ?? -cards.Height;
        }
    }

    // The cards of a move: the ones of the move before keep their controls and a card that left gives its
    // controls to the one that came in.
    void Place(int begun)
    {
        var ids = new int?[placed.Length];
        for (var place = 0; place < ids.Length; place++) ids[place] = cards.Place(place, begun)?.Id;
        foreach (var (id, card) in shown.ToArray())
        {
            if (ids.Contains(id)) continue;
            shown.Remove(id);
            Children.Remove(card);
            spare.Push(card);
        }
        for (var place = 0; place < ids.Length; place++)
        {
            if (ids[place] is not { } id)
            {
                placed[place] = null;
                continue;
            }
            if (!shown.TryGetValue(id, out var card))
            {
                card = spare.Count > 0 ? spare.Pop() : new PatchCard(grid, cards);
                card.Bind(id);
                shown[id] = card;
                Children.Add(card);
            }
            // A later place lies over an earlier one.
            card.ZIndex = place;
            placed[place] = card;
        }
    }
}

/// <summary>A wrapping row of items, two of which change places every frame: a frame moves those two
/// children of the panel and creates none.</summary>
sealed class ReorderPatch : PatchBox
{
    readonly WrapPanel items;
    readonly int count;
    int swapped = -1;

    public ReorderPatch(PatchGrid grid) : base(grid)
    {
        count = ReorderCount(grid.Tiles.Count);
        items = new WrapPanel { ItemSpacing = Gap, LineSpacing = Gap };
        for (var item = 0; item < count; item++)
        {
            var tile = new TileView(
                grid.Look, ReorderWidth(ReorderUnits(item), (float)grid.TileWidth), grid.TileHeight, grid.TileHeight,
                grid.TileHeight * 0.06);
            tile.Bind(item);
            tile.Background = Look.Palette[item % Look.Palette.Length];
            items.Children.Add(tile);
        }
        Children.Add(new Border { Width = grid.Width, Height = grid.Height, Padding = new Thickness(Gap), Child = items });
        Show(0);
        Follow(Show);
    }

    void Show(int frame)
    {
        if (count < 2) return;
        var swap = ReorderSwap(frame, count);
        if (swap == swapped) return;
        if (swapped >= 0) items.Children.Move(swapped, swapped + 1);
        items.Children.Move(swap, swap + 1);
        swapped = swap;
    }
}

/// <summary>Nests of boxes around digits that change in turn: a frame sets the text of one row of nests.</summary>
sealed class DeepPatch : PatchBox
{
    readonly TextBlock[] numbers;
    readonly InTurn turn;

    public DeepPatch(PatchGrid grid) : base(grid)
    {
        var (columns, rows) = Pairs(grid.Tiles.Columns, grid.Tiles.Rows);
        turn = new InTurn(columns, rows);
        numbers = new TextBlock[columns * rows];
        var width = Double((float)grid.TileWidth);
        var height = Double((float)grid.TileHeight);
        for (var index = 0; index < numbers.Length; index++)
        {
            var number = grid.Look.Text(grid.TileHeight * 0.30, Brushes.White, bold: true);
            number.Text = Digits(DeepNumber(index, 0, turn));
            numbers[index] = number;
            Control inner = number;
            for (var level = Depth - 1; level >= 0; level--)
            {
                inner = new Border
                {
                    Background = Look.Palette[DeepColor(index, level)],
                    Padding = new Thickness(grid.TileHeight * 0.06),
                    Child = inner,
                };
            }
            inner.Width = width;
            inner.Height = height;
            var at = PatchGrid.Cell(index, columns, width, height);
            SetLeft(inner, at.X);
            SetTop(inner, at.Y);
            Children.Add(inner);
        }
        Follow(Show);
    }

    static string Digits(int number) => number.ToString("D3");

    void Show(int frame)
    {
        var first = turn.ChangedRow(frame) * turn.Columns;
        for (var index = first; index < first + turn.Columns; index++)
        {
            numbers[index].Text = Digits(DeepNumber(index, frame, turn));
        }
    }
}

/// <summary>A list of rows of tiles that scrolls by one row every frame. The list creates the rows on the
/// screen and recycles each row that leaves for the row that comes in, which rebinds its tiles.</summary>
sealed class ScrollPatch : PatchBox
{
    readonly ScrollViewer scroller;
    readonly int total;
    readonly int visible;
    readonly double pitch;

    public ScrollPatch(PatchGrid grid) : base(grid)
    {
        total = ScrollRows(grid.Tiles.Columns);
        visible = grid.Tiles.Rows;
        pitch = grid.TileHeight + Gap;
        var rows = new TileRows(grid)
        {
            ItemsSource = Enumerable.Range(0, total).ToArray(),
            ItemsPanel = new FuncTemplate<Panel?>(() => new VirtualizingStackPanel()),
            Padding = new Thickness(0, Gap, 0, 0),
        };
        scroller = new ScrollViewer
        {
            Width = grid.Width,
            Height = grid.Height,
            Content = rows,
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        };
        Children.Add(scroller);
        Follow(Show);
    }

    void Show(int frame) => scroller.Offset = new Vector(0, ScrollFirst(frame, total, visible) * pitch);

    sealed class TileRows(PatchGrid grid) : ItemsControl
    {
        protected override Type StyleKeyOverride => typeof(ItemsControl);

        protected override bool NeedsContainerOverride(object? item, int index, out object? recycleKey)
        {
            recycleKey = null;
            return true;
        }

        protected override Control CreateContainerForItemOverride(object? item, int index, object? recycleKey) =>
            new TileRow(grid);

        protected override void PrepareContainerForItemOverride(Control container, object? item, int index)
        {
            base.PrepareContainerForItemOverride(container, item, index);
            ((TileRow)container).Bind(index);
        }
    }

    sealed class TileRow : StackPanel
    {
        readonly TileView[] tiles;

        public TileRow(PatchGrid grid)
        {
            Orientation = Orientation.Horizontal;
            Spacing = Gap;
            Margin = new Thickness(Gap, 0, 0, Gap);
            tiles = new TileView[grid.Tiles.Columns];
            for (var column = 0; column < tiles.Length; column++)
            {
                tiles[column] = new TileView(grid.Look, grid.TileWidth, grid.TileHeight, grid.TileHeight, grid.TileHeight * 0.06);
                Children.Add(tiles[column]);
            }
        }

        public void Bind(int row)
        {
            for (var column = 0; column < tiles.Length; column++)
            {
                var index = row * tiles.Length + column;
                tiles[column].Bind(index);
                tiles[column].Background = Look.Palette[index % Look.Palette.Length];
            }
        }
    }
}

/// <summary>One paragraph, one word of which changes every frame. The paragraph's characters live in a buffer
/// that a frame patches at the word that changed and at the one that changed before.</summary>
sealed class ParagraphPatch : PatchBox
{
    readonly TextBlock text;
    readonly char[] characters;
    readonly int words;
    int changed;

    public ParagraphPatch(PatchGrid grid) : base(grid)
    {
        words = ParagraphWords(grid.Tiles.Count);
        var first = Paragraph(0, words);
        characters = first.ToCharArray();
        changed = ParagraphChanged(0, words);
        text = grid.Look.Text(grid.TileHeight * 0.30, Brushes.White);
        text.LineHeight = text.FontSize * TextLeading.Em;
        text.Text = first;
        text.Width = grid.Width - 2 * Gap;
        text.TextWrapping = TextWrapping.Wrap;
        SetLeft(text, Gap);
        SetTop(text, Gap);
        Children.Add(text);
        Follow(Show);
    }

    void Show(int frame)
    {
        Write(changed, changed);
        changed = ParagraphChanged(frame, words);
        Write(changed, frame % 100_000);
        text.Text = new string(characters);
    }

    // The word at `word` shows `number` in five digits.
    void Write(int word, int number)
    {
        number.TryFormat(characters.AsSpan(word * 6, 5), out _, "D5");
    }
}

/// <summary>Translucent tiles lying over one another, one of which is opaque. A frame sets the fill of
/// the tile that was opaque and of the one that is.</summary>
sealed class PilePatch : PatchBox
{
    static readonly IBrush[] Translucent = PerfData.PaletteArgb
        .Select(argb => (IBrush)new ImmutableSolidColorBrush(Color.FromUInt32(argb & 0xFFFFFF | (uint)PileAlpha << 24)))
        .ToArray();
    static readonly IBrush Opaque = new ImmutableSolidColorBrush(Color.FromUInt32(PileOpaqueArgb));

    readonly TileView[] tiles;
    int opaque;

    public PilePatch(PatchGrid grid) : base(grid)
    {
        var (columns, rows) = PileGrid(grid.Tiles.Columns, grid.Tiles.Rows);
        tiles = new TileView[columns * rows];
        for (var index = 0; index < tiles.Length; index++)
        {
            var tile = new TileView(
                grid.Look, Double((float)grid.TileWidth), Double((float)grid.TileHeight), grid.TileHeight, 0);
            tile.Bind(index);
            tile.Background = Translucent[index % Translucent.Length];
            var at = PatchGrid.Cell(index, columns, grid.TileWidth, grid.TileHeight);
            SetLeft(tile, at.X);
            SetTop(tile, at.Y);
            Children.Add(tile);
            tiles[index] = tile;
        }
        opaque = PileOpaque(0, tiles.Length);
        tiles[opaque].Background = Opaque;
        Follow(Show);
    }

    void Show(int frame)
    {
        tiles[opaque].Background = Translucent[opaque % Translucent.Length];
        opaque = PileOpaque(frame, tiles.Length);
        tiles[opaque].Background = Opaque;
    }
}
