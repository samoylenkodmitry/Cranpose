// The gauntlet in Avalonia: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in
// Avalonia's own idiom. The benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. The top level's animation frame callback advances it once
// per frame; the attached controls read it and set only what changed.

using System.Runtime.InteropServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Immutable;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using PerfCompare;
using static PerfCompare.PerfData;
// Android's implicit usings bring their own `Orientation`.
using Orientation = Avalonia.Layout.Orientation;

namespace PerfAvalonia;

/// <summary>The frame index every per-frame value follows.</summary>
public static class Clock
{
    public static int Frame { get; private set; }

    /// <summary>Raised once per frame, after the index advanced.</summary>
    public static event Action<int>? Advanced;

    public static int Advance()
    {
        Frame += 1;
        Advanced?.Invoke(Frame);
        return Frame;
    }

    /// <summary>Calls `onFrame` every frame while `control` is on screen.</summary>
    public static void Follow(Control control, Action<int> onFrame)
    {
        control.AttachedToVisualTree += (_, _) => Advanced += onFrame;
        control.DetachedFromVisualTree += (_, _) => Advanced -= onFrame;
    }
}

/// <summary>Colors and text the gauntlet's controls share at one tier's scale.</summary>
public sealed class Look(double s)
{
    public static readonly IBrush Ink = Brush(0xFF111827);
    public static readonly IBrush Body = Brush(0xFF374151);
    public static readonly IBrush Muted = Brush(0xFF6B7280);
    public static readonly IBrush Hairline = Brush(0xFFE5E7EB);
    public static readonly IBrush Panel = Brush(0xFFE2E8F0);
    public static readonly IBrush Up = Brush(0xFF16A34A);
    public static readonly IBrush Down = Brush(0xFFDC2626);
    public static readonly IBrush[] LevelBackground = [Brush(0xFFF1F5F9), Brush(0xFFCBD5E1)];
    public static readonly IBrush[] Palette = PaletteArgb.Select(Brush).ToArray();
    public static readonly IBrush[] ChipBackground = ChipBackgroundArgb.Select(Brush).ToArray();
    public static readonly IBrush[] GradientEnd = GradientEndArgb.Select(Brush).ToArray();

    public double S { get; } = s;

    static IBrush Brush(uint argb) => new ImmutableSolidColorBrush(Color.FromUInt32(argb));

    public TextBlock Text(double size, IBrush color, bool bold = false) => new()
    {
        FontSize = size * S,
        Foreground = color,
        FontWeight = bold ? FontWeight.Bold : FontWeight.Normal,
    };

    /// <summary>Lines 1.4 em apart, cut with an ellipsis after `maxLines`.</summary>
    public TextBlock Paragraph(double size, IBrush color, int maxLines, bool bold = false)
    {
        var text = Text(size, color, bold);
        text.LineHeight = size * S * 1.4;
        text.MaxLines = maxLines;
        text.TextWrapping = TextWrapping.Wrap;
        text.TextTrimming = TextTrimming.CharacterEllipsis;
        return text;
    }

    public static Border Rounded(IBrush? background, double radius, Thickness padding, Control child) => new()
    {
        Background = background,
        CornerRadius = new CornerRadius(radius),
        Padding = padding,
        Child = child,
    };

    /// <summary>`count` equal columns `gap` apart: child `n` goes in column `2n`.</summary>
    public static Grid Columns(int count, double gap)
    {
        var grid = new Grid();
        for (var column = 0; column < count; column++)
        {
            if (column > 0) grid.ColumnDefinitions.Add(new ColumnDefinition(gap, GridUnitType.Pixel));
            grid.ColumnDefinitions.Add(new ColumnDefinition(1, GridUnitType.Star));
        }
        return grid;
    }

    public static Grid Place(Grid grid, Control child, int column)
    {
        Grid.SetColumn(child, column);
        grid.Children.Add(child);
        return grid;
    }
}

public sealed class GauntletView : Grid
{
    /// <summary>Blocks of five card rows and a cluster: no measurement window reaches the end.</summary>
    const int Blocks = 2000;

    /// <summary>How far the list scrolls each frame, in dp.</summary>
    const double ScrollPerFrame = 3;

    readonly int freeze;
    readonly Grid content;
    readonly ScrollViewer scroller;
    bool firstFrameLogged;

    public GauntletView(int tierIndex, int freeze)
    {
        this.freeze = freeze;
        var tier = GauntletTier(tierIndex);
        var look = new Look(tier.Scale);
        var s = look.S;
        TextElement.SetFontFamily(this, new FontFamily(DeviceRoboto.Family));
        Background = new ImmutableSolidColorBrush(Color.FromUInt32(0xFFEEF0F5));
        RowDefinitions = new RowDefinitions("56,*");

        var title = new Look(1).Text(20, Brushes.White, bold: true);
        title.Text = "Gauntlet";
        title.VerticalAlignment = VerticalAlignment.Center;
        var topBar = new Border
        {
            Background = new ImmutableSolidColorBrush(Color.FromUInt32(0xFF1E2A4A)),
            Padding = new Thickness(16, 0),
            Child = title,
        };
        Children.Add(topBar);

        var tiles = new WrapPanel { ItemSpacing = 4 * s, LineSpacing = 4 * s };
        var quotes = Tickers(tier.Tickers);
        for (var index = 0; index < quotes.Length; index++)
        {
            tiles.Children.Add(new TickerTile(look, quotes[index], index));
        }
        var panel = new Border { Background = Look.Panel, Padding = new Thickness(6 * s), Child = tiles };

        var posts = Posts();
        var avatars = Enumerable.Range(0, AvatarCount).Select(Avatar).ToArray();
        var rows = new Rows(look, tier.Columns, tier.Depth, posts, avatars)
        {
            ItemsSource = Enumerable.Range(0, Blocks * (CardRowsPerCluster + 1)).ToArray(),
            ItemsPanel = new FuncTemplate<Panel?>(() => new VirtualizingStackPanel()),
            Padding = new Thickness(0, 8 * s, 0, 0),
        };
        scroller = new ScrollViewer
        {
            Content = rows,
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        };

        content = new Grid
        {
            HorizontalAlignment = HorizontalAlignment.Left,
            RowDefinitions = new RowDefinitions("Auto,*"),
        };
        content.Children.Add(panel);
        // The panels stack over the list and show only over it, as every app clips them.
        var area = new Grid { ClipToBounds = true };
        area.Children.Add(scroller);
        for (var layer = 0; layer < tier.Layers; layer++) area.Children.Add(new StackedLayer(layer, tier.LayerRows));
        SetRow(area, 1);
        content.Children.Add(area);
        SetRow(content, 1);
        Children.Add(content);
    }

    static Bitmap Avatar(int index)
    {
        var bitmap = new WriteableBitmap(
            new PixelSize(AvatarSize, AvatarSize), new Vector(96, 96), PixelFormat.Rgba8888, AlphaFormat.Opaque);
        using var buffer = bitmap.Lock();
        var rgba = AvatarRgba(index);
        var row = AvatarSize * 4;
        for (var y = 0; y < AvatarSize; y++)
        {
            Marshal.Copy(rgba, y * row, buffer.Address + y * buffer.RowBytes, row);
        }
        return bitmap;
    }

    protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e)
    {
        base.OnAttachedToVisualTree(e);
        var topLevel = TopLevel.GetTopLevel(this);
        // Avalonia shows the system bars and draws behind them unless told
        // otherwise: the window is fullscreen and starts below the display
        // cutout, as the other apps' windows do.
        if (topLevel?.InsetsManager is { } insets)
        {
            insets.IsSystemBarVisible = false;
            insets.DisplayEdgeToEdgePreference = false;
        }
        topLevel?.RequestAnimationFrame(OnFrame);
    }

    void OnFrame(TimeSpan _)
    {
        if (!firstFrameLogged)
        {
            firstFrameLogged = true;
            Launch.Log("PERF first_frame");
        }
        var frame = Clock.Advance();
        var topLevel = TopLevel.GetTopLevel(this);
        var scaling = topLevel?.RenderScaling ?? 1;
        // The width is set once per frame: Avalonia measures everything below again.
        content.Width = Math.Round(Bounds.Width * scaling * WidthFraction(frame)) / scaling;
        scroller.Offset = new Vector(0, frame * ScrollPerFrame);
        if (freeze > 0 && frame >= freeze)
        {
            Launch.Log($"PERF frozen frame={frame}");
            return;
        }
        topLevel?.RequestAnimationFrame(OnFrame);
    }
}

/// <summary>
/// The list: a row of cards, or a cluster after every five. Each kind recycles
/// only its own rows, which rebind to the row they show.
/// </summary>
sealed class Rows(Look look, int columns, int depth, Post[] posts, Bitmap[] avatars) : ItemsControl
{
    static readonly object CardKey = new();
    static readonly object ClusterKey = new();

    protected override Type StyleKeyOverride => typeof(ItemsControl);

    static bool IsCluster(int row) => row % (CardRowsPerCluster + 1) == CardRowsPerCluster;

    protected override bool NeedsContainerOverride(object? item, int index, out object? recycleKey)
    {
        recycleKey = IsCluster(index) ? ClusterKey : CardKey;
        return true;
    }

    protected override Control CreateContainerForItemOverride(object? item, int index, object? recycleKey) =>
        recycleKey == ClusterKey ? new ClusterRow(look, depth) : new CardRow(look, columns, posts, avatars);

    protected override void PrepareContainerForItemOverride(Control container, object? item, int index)
    {
        base.PrepareContainerForItemOverride(container, item, index);
        var block = index / (CardRowsPerCluster + 1);
        switch (container)
        {
            case CardRow cards:
                cards.Bind((block * CardRowsPerCluster + index % (CardRowsPerCluster + 1)) * columns);
                break;
            case ClusterRow cluster:
                cluster.Bind(block);
                break;
        }
    }
}

/// <summary>A rounded track filled to its share of the frame.</summary>
/// <summary>A translucent panel stacked over the list. Its controls never change: each frame sets
/// only the panel's translation and tilt and its nested card's tilt, which the compositor applies
/// to their retained drawing.</summary>
sealed class StackedLayer : Border
{
    static readonly IBrush LayerBackground = new ImmutableSolidColorBrush(Color.FromUInt32(0xC01E293B));
    static readonly IBrush NestedBackground = new ImmutableSolidColorBrush(Color.FromUInt32(0xE6FFFFFF));

    readonly int layer;
    readonly RotateTransform tilt = new();
    readonly TranslateTransform shift = new();
    readonly RotateTransform nestedTilt = new();

    public StackedLayer(int layer, int rows)
    {
        this.layer = layer;
        var look = new Look(1);
        Width = 220;
        HorizontalAlignment = HorizontalAlignment.Left;
        VerticalAlignment = VerticalAlignment.Top;
        Background = LayerBackground;
        CornerRadius = new CornerRadius(12);
        Padding = new Thickness(10);
        var title = look.Text(13, Brushes.White, bold: true);
        title.Text = $"Layer {layer + 1}";
        var cells = new StackPanel { Spacing = 3 };
        for (var row = 0; row < rows; row++)
        {
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 3 };
            for (var column = 0; column < LayerColumns; column++)
            {
                var cell = row * LayerColumns + column;
                var label = look.Text(9, Brushes.White);
                label.Text = $"{cell + 1}";
                label.HorizontalAlignment = HorizontalAlignment.Center;
                label.VerticalAlignment = VerticalAlignment.Center;
                var box = Look.Rounded(Look.Palette[LayerCellColor(layer, cell)], 4, new Thickness(0), label);
                box.Width = 22;
                box.Height = 22;
                line.Children.Add(box);
            }
            cells.Children.Add(line);
        }
        var heading = look.Text(11, Look.Ink, bold: true);
        heading.Text = $"Nested in layer {layer + 1}";
        var body = look.Text(11, Look.Body);
        body.Text = "Tilts against its panel";
        var nested = Look.Rounded(NestedBackground, 8, new Thickness(8),
            new StackPanel { Spacing = 2, Children = { heading, body } });
        nested.RenderTransform = nestedTilt;
        Child = new StackPanel { Spacing = 8, Children = { title, cells, nested } };
        // Tilted about its center, then moved.
        RenderTransform = new TransformGroup { Children = { tilt, shift } };
        OnFrame(0);
        Clock.Follow(this, OnFrame);
    }

    void OnFrame(int frame)
    {
        var degrees = LayerDegrees(layer, frame);
        tilt.Angle = degrees;
        shift.X = LayerX(layer, frame);
        shift.Y = LayerY(layer, frame);
        nestedTilt.Angle = -degrees;
    }
}

sealed class Bar(double radius, Func<int, double> share) : Control
{
    public IBrush Fill { get; set; } = Brushes.Black;

    public override void Render(DrawingContext context)
    {
        var track = new Rect(Bounds.Size);
        context.DrawRectangle(Look.Hairline, null, track, radius, radius);
        context.DrawRectangle(Fill, null, track.WithWidth(track.Width * share(Clock.Frame)), radius, radius);
    }
}

sealed class TickerTile : Border
{
    readonly Ticker quote;
    readonly TextBlock price;
    readonly TextBlock change;
    readonly Bar bar;

    public TickerTile(Look look, Ticker quote, int index)
    {
        this.quote = quote;
        var s = look.S;
        var symbol = look.Text(10, Look.Ink, bold: true);
        symbol.Text = quote.Symbol;
        price = look.Text(10, Look.Body);
        change = look.Text(10, Look.Up);
        bar = new Bar(2 * s, Share) { Width = 20 * s, Height = 4 * s, Fill = Look.Palette[index % Look.Palette.Length] };
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 * s };
        foreach (var control in new Control[] { symbol, price, change, bar })
        {
            control.VerticalAlignment = VerticalAlignment.Center;
            row.Children.Add(control);
        }
        Background = Brushes.White;
        CornerRadius = new CornerRadius(6 * s);
        Padding = new Thickness(6 * s, 3 * s);
        Child = row;
        OnFrame(Clock.Frame);
        Clock.Follow(this, OnFrame);
    }

    double Share(int frame) => Math.Clamp(
        (TickerCents(quote, frame) - quote.BaseCents + quote.SwingCents) / (2.0 * quote.SwingCents), 0, 1);

    void OnFrame(int frame)
    {
        var cents = TickerCents(quote, frame);
        price.Text = CentsText(cents);
        change.Text = ChangeText(quote, cents);
        change.Foreground = cents >= quote.BaseCents ? Look.Up : Look.Down;
        bar.InvalidateVisual();
    }
}

/// <summary>A row of cards, rebound as the list reuses it.</summary>
sealed class CardRow : Grid
{
    readonly CardView[] cards;
    readonly Post[] posts;
    readonly Bitmap[] avatars;

    public CardRow(Look look, int columns, Post[] posts, Bitmap[] avatars)
    {
        this.posts = posts;
        this.avatars = avatars;
        var s = look.S;
        Margin = new Thickness(8 * s, 0, 8 * s, 8 * s);
        cards = new CardView[columns];
        for (var column = 0; column < columns; column++)
        {
            if (column > 0) ColumnDefinitions.Add(new ColumnDefinition(8 * s, GridUnitType.Pixel));
            ColumnDefinitions.Add(new ColumnDefinition(1, GridUnitType.Star));
            cards[column] = new CardView(look) { VerticalAlignment = VerticalAlignment.Top };
            Look.Place(this, cards[column], column * 2);
        }
    }

    public void Bind(int first)
    {
        for (var column = 0; column < cards.Length; column++)
        {
            var card = first + column;
            cards[column].Bind(posts[card % PostCount], avatars[card % AvatarCount], card);
        }
    }
}

sealed class CardView : Panel
{
    static readonly string[] FooterLabels = ["likes", "replies", "shares"];

    readonly Image avatar;
    readonly TextBlock title;
    readonly Run author;
    readonly Run tag;
    readonly TextBlock body;
    readonly Bar progress;
    readonly TextBlock percent;
    readonly Sparkline sparkline;
    readonly (Border Chip, TextBlock Text)[] chips;
    readonly TextBlock[] stats;
    readonly Border badge;
    readonly RotateTransform tilt = new();
    int card;

    public CardView(Look look)
    {
        var s = look.S;
        avatar = new Image { Width = 32 * s, Height = 32 * s, Stretch = Stretch.UniformToFill };
        var avatarClip = new Border
        {
            CornerRadius = new CornerRadius(16 * s),
            ClipToBounds = true,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 8 * s, 0),
            Child = avatar,
        };
        title = look.Paragraph(13, Look.Ink, 2, bold: true);
        author = new Run { FontWeight = FontWeight.Bold };
        tag = new Run();
        var subtitle = look.Text(11, Look.Muted);
        subtitle.MaxLines = 1;
        subtitle.TextTrimming = TextTrimming.CharacterEllipsis;
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        subtitle.Inlines = [new Run("by "), author, new Run(" · "), tag];
        var headerText = new StackPanel { VerticalAlignment = VerticalAlignment.Center, Children = { title, subtitle } };
        var header = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*") };
        header.Children.Add(avatarClip);
        Look.Place(header, headerText, 1);

        body = look.Paragraph(12, Look.Body, 4);

        progress = new Bar(3 * s, frame => ProgressPermille(card, frame) / 1000.0)
        {
            Height = 6 * s,
            VerticalAlignment = VerticalAlignment.Center,
        };
        percent = look.Text(10, Look.Muted);
        percent.VerticalAlignment = VerticalAlignment.Center;
        percent.Margin = new Thickness(6 * s, 0, 0, 0);
        var progressRow = new Grid { ColumnDefinitions = new ColumnDefinitions("*,Auto") };
        progressRow.Children.Add(progress);
        Look.Place(progressRow, percent, 1);

        sparkline = new Sparkline(1.5 * s) { Height = 36 * s };

        var chipFlow = new WrapPanel { ItemSpacing = 4 * s, LineSpacing = 4 * s };
        chips = new (Border, TextBlock)[3];
        for (var index = 0; index < chips.Length; index++)
        {
            var text = look.Text(10, Look.Ink);
            var chip = Look.Rounded(null, 10 * s, new Thickness(8 * s, 3 * s), text);
            chipFlow.Children.Add(chip);
            chips[index] = (chip, text);
        }

        // Three counters split by dividers as tall as the tallest counter:
        // the grid's row is as tall as its tallest cell.
        var footer = Look.Columns(FooterLabels.Length, 1);
        stats = new TextBlock[FooterLabels.Length];
        for (var index = 0; index < FooterLabels.Length; index++)
        {
            if (index > 0) Look.Place(footer, new Border { Background = Look.Hairline }, index * 2 - 1);
            stats[index] = look.Text(12, Look.Ink, bold: true);
            stats[index].HorizontalAlignment = HorizontalAlignment.Center;
            var label = look.Text(9, Look.Muted);
            label.Text = FooterLabels[index];
            label.HorizontalAlignment = HorizontalAlignment.Center;
            Look.Place(footer, new StackPanel { Children = { stats[index], label } }, index * 2);
        }

        var column = new StackPanel
        {
            Spacing = 6 * s,
            Children = { header, body, progressRow, sparkline, chipFlow, footer },
        };
        Children.Add(new Border
        {
            Background = Brushes.White,
            BorderBrush = Look.Hairline,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(12 * s),
            // Compose draws its border over the padding; an Avalonia border
            // insets the content by both, so the padding gives the border's
            // width back.
            Padding = new Thickness(10 * s - 1),
            BoxShadow = new BoxShadows(new BoxShadow
            {
                OffsetY = s,
                Blur = 3 * s,
                Color = Color.FromArgb(0x3D, 0, 0, 0),
            }),
            Child = column,
        });

        var hot = look.Text(9, Brushes.White, bold: true);
        hot.Text = "HOT";
        badge = Look.Rounded(Look.Palette[0], 8 * s, new Thickness(6 * s, 2 * s), hot);
        badge.HorizontalAlignment = HorizontalAlignment.Right;
        badge.VerticalAlignment = VerticalAlignment.Top;
        badge.Margin = new Thickness(6 * s);
        badge.Opacity = 0.9;
        // A translucent tag tilting with the frame: rotated, never measured again.
        badge.RenderTransform = tilt;
        Children.Add(badge);
        Clock.Follow(this, OnFrame);
    }

    public void Bind(Post post, Bitmap image, int card)
    {
        this.card = card;
        avatar.Source = image;
        title.Text = post.Title;
        author.Text = post.Author;
        var (firstTag, tagColor) = post.Tags[0];
        tag.Text = firstTag;
        tag.Foreground = Look.GradientEnd[tagColor];
        body.Text = post.Body;
        progress.Fill = Look.Palette[card % Look.Palette.Length];
        sparkline.Bind(card, PaletteArgb[post.Color]);
        for (var index = 0; index < chips.Length; index++)
        {
            var (text, color) = post.Tags[index];
            chips[index].Chip.Background = Look.ChipBackground[color];
            chips[index].Text.Text = text;
            chips[index].Text.Foreground = Look.GradientEnd[color];
        }
        for (var index = 0; index < stats.Length; index++) stats[index].Text = post.Stats[index];
        badge.IsVisible = card % 5 == 0;
        OnFrame(Clock.Frame);
    }

    void OnFrame(int frame)
    {
        percent.Text = $"{ProgressPermille(card, frame) / 10}%";
        progress.InvalidateVisual();
        sparkline.InvalidateVisual();
        if (badge.IsVisible) tilt.Angle = BadgeDegrees(card, frame);
    }
}

/// <summary>A card's 48-point line over a fading fill, drawn from the frame.</summary>
sealed class Sparkline(double strokeWidth) : Control
{
    int card;
    uint argb;
    IPen line = new ImmutablePen(Brushes.Black);
    IBrush fill = Brushes.Transparent;

    public void Bind(int card, uint argb)
    {
        this.card = card;
        if (this.argb == argb) return;
        this.argb = argb;
        var color = Color.FromUInt32(argb);
        line = new ImmutablePen(new ImmutableSolidColorBrush(color), strokeWidth);
        fill = new ImmutableLinearGradientBrush(
            [new ImmutableGradientStop(0, Color.FromArgb(0x40, color.R, color.G, color.B)),
                new ImmutableGradientStop(1, Color.FromArgb(0, color.R, color.G, color.B))],
            startPoint: new RelativePoint(0, 0, RelativeUnit.Relative),
            endPoint: new RelativePoint(0, 1, RelativeUnit.Relative));
    }

    public override void Render(DrawingContext context)
    {
        var frame = Clock.Frame;
        var (width, height) = (Bounds.Width, Bounds.Height);
        var step = width / (SparkPoints - 1);
        var area = new StreamGeometry();
        var stroke = new StreamGeometry();
        using (var areaPath = area.Open())
        using (var strokePath = stroke.Open())
        {
            areaPath.BeginFigure(new Point(0, height), isFilled: true);
            for (var index = 0; index < SparkPoints; index++)
            {
                var point = new Point(index * step, height * (1 - SparkValue(card, index, frame)));
                areaPath.LineTo(point);
                if (index == 0) strokePath.BeginFigure(point, isFilled: false);
                else strokePath.LineTo(point);
            }
            areaPath.LineTo(new Point(width, height));
            areaPath.EndFigure(isClosed: true);
            strokePath.EndFigure(isClosed: false);
        }
        context.DrawGeometry(fill, null, area);
        context.DrawGeometry(null, line, stroke);
    }
}

/// <summary>
/// `depth` levels nested inside one another, each a row of three labels above
/// the next level.
/// </summary>
sealed class ClusterRow : Decorator
{
    readonly Level level;

    public ClusterRow(Look look, int depth)
    {
        Margin = new Thickness(8 * look.S, 0, 8 * look.S, 8 * look.S);
        level = new Level(look, depth);
        Child = level.View;
    }

    public void Bind(int cluster) => level.Bind(cluster);

    sealed class Level
    {
        readonly int remaining;
        readonly (Border Chip, TextBlock Text)[] chips = new (Border, TextBlock)[3];
        readonly Level? next;

        public Border View { get; }

        public Level(Look look, int remaining)
        {
            this.remaining = remaining;
            var s = look.S;
            var row = Look.Columns(chips.Length, 2 * s);
            for (var chip = 0; chip < chips.Length; chip++)
            {
                var text = look.Text(9, Brushes.White);
                text.TextWrapping = TextWrapping.Wrap;
                var border = Look.Rounded(null, 2 * s, new Thickness(0), text);
                border.VerticalAlignment = VerticalAlignment.Top;
                Look.Place(row, border, chip * 2);
                chips[chip] = (border, text);
            }
            var stack = new StackPanel { Spacing = s, Children = { row } };
            if (remaining > 0)
            {
                next = new Level(look, remaining - 1);
                stack.Children.Add(next.View);
            }
            View = Look.Rounded(Look.LevelBackground[remaining % 2], 4 * s, new Thickness(3 * s, s, s, s), stack);
        }

        public void Bind(int cluster)
        {
            for (var chip = 0; chip < chips.Length; chip++)
            {
                chips[chip].Text.Text = $"C{cluster}.L{remaining}.{chip}";
                chips[chip].Chip.Background = Look.Palette[(remaining + chip + cluster) % Look.Palette.Length];
            }
            next?.Bind(cluster);
        }
    }
}
