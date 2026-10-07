// The gauntlet in Uno Platform: the screen `compose-app/.../Gauntlet.kt`
// draws, element for element, in WinUI's own idiom on Uno's Skia renderer:
// panels and text blocks, an ItemsRepeater whose element factory recycles
// each kind of row, and a Skia canvas element for the sparklines. The
// benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. CompositionTarget.Rendering advances it once per frame;
// the elements on screen set only what changed.

using System.Numerics;
using System.Runtime.InteropServices.WindowsRuntime;
using Microsoft.UI;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Documents;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Xaml.Shapes;
using PerfCompare;
using SkiaSharp;
using Uno.WinUI.Graphics2DSK;
using Windows.Foundation;
using Windows.UI;
using static PerfCompare.PerfData;

namespace PerfUno;

/// <summary>What `am start` or `desktop.py` asked of the gauntlet.</summary>
public static class Launch
{
    /// <summary>Load tier, 1 to 16.</summary>
    public static int Tier { get; set; } = 5;

    /// <summary>Stop on this frame and hold still, for picture comparisons; 0 runs on.</summary>
    public static int Freeze { get; set; }

    /// <summary>Writes a `PERF` line where `measure.py` and `desktop.py` read it.</summary>
    public static Action<string> Log { get; set; } = _ => { };
}

public sealed class App : Application
{
    public App() => RequestedTheme = ApplicationTheme.Light;

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Resources.MergedDictionaries.Add(new XamlControlsResources());
        var view = new GauntletView(Launch.Tier, Launch.Freeze);
        var window = new Window { Content = view };
        if (!OperatingSystem.IsAndroid())
        {
            window.Title = "Gauntlet";
            // The window every desktop app opens for the gauntlet, 1280 x 820
            // points of content: Uno's macOS window sizes its content in pixels.
            view.Loaded += (_, _) =>
            {
                var scale = view.XamlRoot?.RasterizationScale ?? 1;
                window.AppWindow.Resize(new Windows.Graphics.SizeInt32
                {
                    Width = (int)Math.Round(1280 * scale),
                    Height = (int)Math.Round(820 * scale),
                });
            };
        }
        window.Activate();
    }
}

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

    /// <summary>Calls `onFrame` every frame while `element` is on screen.</summary>
    public static void Follow(FrameworkElement element, Action<int> onFrame)
    {
        element.Loaded += (_, _) => Advanced += onFrame;
        element.Unloaded += (_, _) => Advanced -= onFrame;
    }
}

/// <summary>Colors and text the gauntlet's elements share at one tier's scale.</summary>
public sealed class Look(double s)
{
    public static readonly Brush Ink = Solid(0xFF111827);
    public static readonly Brush Body = Solid(0xFF374151);
    public static readonly Brush Muted = Solid(0xFF6B7280);
    public static readonly Brush Hairline = Solid(0xFFE5E7EB);
    public static readonly Brush Panel = Solid(0xFFE2E8F0);
    public static readonly Brush Up = Solid(0xFF16A34A);
    public static readonly Brush Down = Solid(0xFFDC2626);
    public static readonly Brush White = Solid(0xFFFFFFFF);
    public static readonly Brush[] LevelBackground = [Solid(0xFFF1F5F9), Solid(0xFFCBD5E1)];
    public static readonly Brush[] Palette = PaletteArgb.Select(Solid).ToArray();
    public static readonly Brush[] ChipBackground = ChipBackgroundArgb.Select(Solid).ToArray();
    public static readonly Brush[] GradientEnd = GradientEndArgb.Select(Solid).ToArray();

    /// <summary>Roboto: the device's own on Android, elsewhere the files every desktop app loads.</summary>
    static readonly FontFamily Regular = new(OperatingSystem.IsAndroid()
        ? "Roboto" : "ms-appx:///Assets/Fonts/Roboto-Regular.ttf#Roboto");
    static readonly FontFamily BoldFamily = new(OperatingSystem.IsAndroid()
        ? "Roboto" : "ms-appx:///Assets/Fonts/Roboto-Bold.ttf#Roboto");

    public double S { get; } = s;

    public static Color ColorOf(uint argb) =>
        Color.FromArgb((byte)(argb >> 24), (byte)(argb >> 16), (byte)(argb >> 8), (byte)argb);

    static Brush Solid(uint argb) => new SolidColorBrush(ColorOf(argb));

    public static void Style(TextElement text, bool bold)
    {
        text.FontFamily = bold ? BoldFamily : Regular;
        text.FontWeight = bold ? FontWeights.Bold : FontWeights.Normal;
    }

    public TextBlock Text(double size, Brush color, bool bold = false)
    {
        var text = new TextBlock { FontSize = size * S, Foreground = color };
        text.FontFamily = bold ? BoldFamily : Regular;
        text.FontWeight = bold ? FontWeights.Bold : FontWeights.Normal;
        return text;
    }

    /// <summary>Lines 1.4 em apart, cut with an ellipsis after `maxLines`.</summary>
    public TextBlock Paragraph(double size, Brush color, int maxLines, bool bold = false)
    {
        var text = Text(size, color, bold);
        text.LineHeight = size * S * 1.4;
        text.LineStackingStrategy = LineStackingStrategy.BlockLineHeight;
        text.MaxLines = maxLines;
        text.TextWrapping = TextWrapping.Wrap;
        text.TextTrimming = TextTrimming.CharacterEllipsis;
        return text;
    }

    public static Border Rounded(Brush? background, double radius, Thickness padding, UIElement child) => new()
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
            if (column > 0) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(gap) });
            grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        }
        return grid;
    }

    public static Grid Place(Grid grid, FrameworkElement child, int column)
    {
        Grid.SetColumn(child, column);
        grid.Children.Add(child);
        return grid;
    }
}

/// <summary>Children left to right, wrapping, `Spacing` apart both ways: Compose's FlowRow.</summary>
public sealed class FlowPanel : Panel
{
    public double Spacing { get; init; }

    protected override Size MeasureOverride(Size available)
    {
        double x = 0, y = 0, line = 0;
        foreach (var child in Children)
        {
            child.Measure(new Size(double.PositiveInfinity, double.PositiveInfinity));
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > available.Width)
            {
                y += line + Spacing;
                x = 0;
                line = 0;
            }
            x += size.Width + Spacing;
            line = Math.Max(line, size.Height);
        }
        return new Size(double.IsInfinity(available.Width) ? x : available.Width, y + line);
    }

    protected override Size ArrangeOverride(Size final)
    {
        double x = 0, y = 0, line = 0;
        foreach (var child in Children)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > final.Width)
            {
                y += line + Spacing;
                x = 0;
                line = 0;
            }
            child.Arrange(new Rect(x, y, size.Width, size.Height));
            x += size.Width + Spacing;
            line = Math.Max(line, size.Height);
        }
        return final;
    }
}

/// <summary>A rounded track filled to a share: the fill takes the share of a two-column grid.</summary>
public sealed class Bar : Border
{
    readonly ColumnDefinition filled = new() { Width = new GridLength(0, GridUnitType.Star) };
    readonly ColumnDefinition rest = new() { Width = new GridLength(1, GridUnitType.Star) };
    readonly Border fill;

    public Bar(double radius)
    {
        Background = Look.Hairline;
        CornerRadius = new CornerRadius(radius);
        fill = new Border { CornerRadius = new CornerRadius(radius) };
        var grid = new Grid();
        grid.ColumnDefinitions.Add(filled);
        grid.ColumnDefinitions.Add(rest);
        grid.Children.Add(fill);
        Child = grid;
    }

    public Brush Fill
    {
        set => fill.Background = value;
    }

    public double Share
    {
        set
        {
            var share = Math.Clamp(value, 0, 1);
            filled.Width = new GridLength(share, GridUnitType.Star);
            rest.Width = new GridLength(1 - share, GridUnitType.Star);
        }
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
        Background = new SolidColorBrush(Look.ColorOf(0xFFEEF0F5));
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(56) });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var title = new Look(1).Text(20, Look.White, bold: true);
        title.Text = "Gauntlet";
        title.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(new Border
        {
            Background = new SolidColorBrush(Look.ColorOf(0xFF1E2A4A)),
            Padding = new Thickness(16, 0, 16, 0),
            Child = title,
        });

        var tiles = new FlowPanel { Spacing = 4 * s };
        var quotes = Tickers(tier.Tickers);
        for (var index = 0; index < quotes.Length; index++)
        {
            tiles.Children.Add(new TickerTile(look, quotes[index], index));
        }
        var panel = new Border { Background = Look.Panel, Padding = new Thickness(6 * s), Child = tiles };

        var posts = Posts();
        var avatars = Enumerable.Range(0, AvatarCount).Select(Avatar).ToArray();
        var rows = new ItemsRepeater
        {
            ItemsSource = Enumerable.Range(0, Blocks * (CardRowsPerCluster + 1)).ToList(),
            ItemTemplate = new RowFactory(look, tier.Columns, tier.Depth, posts, avatars),
            Layout = new StackLayout { Spacing = 8 * s },
            Margin = new Thickness(8 * s),
        };
        scroller = new ScrollViewer
        {
            Content = rows,
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        };

        content = new Grid { HorizontalAlignment = HorizontalAlignment.Left };
        content.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        content.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        content.Children.Add(panel);
        // The panels stack over the list and show only over it, as every app clips them.
        var area = new Grid();
        area.SizeChanged += (_, size) =>
            area.Clip = new RectangleGeometry { Rect = new Rect(0, 0, size.NewSize.Width, size.NewSize.Height) };
        area.Children.Add(scroller);
        for (var layer = 0; layer < tier.Layers; layer++) area.Children.Add(new StackedLayer(layer, tier.LayerRows));
        SetRow(area, 1);
        content.Children.Add(area);
        SetRow(content, 1);
        Children.Add(content);
        Loaded += (_, _) => CompositionTarget.Rendering += OnFrame;
    }

    static ImageSource Avatar(int index)
    {
        var bitmap = new WriteableBitmap(AvatarSize, AvatarSize);
        var rgba = AvatarRgba(index);
        var bgra = new byte[rgba.Length];
        for (var pixel = 0; pixel < rgba.Length; pixel += 4)
        {
            bgra[pixel] = rgba[pixel + 2];
            bgra[pixel + 1] = rgba[pixel + 1];
            bgra[pixel + 2] = rgba[pixel];
            bgra[pixel + 3] = rgba[pixel + 3];
        }
        using (var stream = bitmap.PixelBuffer.AsStream())
        {
            stream.Write(bgra, 0, bgra.Length);
        }
        bitmap.Invalidate();
        return bitmap;
    }

    void OnFrame(object? sender, object e)
    {
        if (!firstFrameLogged)
        {
            firstFrameLogged = true;
            Launch.Log("PERF first_frame");
        }
        var frame = Clock.Advance();
        var scaling = XamlRoot?.RasterizationScale ?? 1;
        // The width is set once per frame: WinUI measures everything below again.
        content.Width = Math.Round(ActualWidth * scaling * WidthFraction(frame)) / scaling;
        scroller.ChangeView(null, frame * ScrollPerFrame, null, disableAnimation: true);
        if (freeze > 0 && frame >= freeze)
        {
            Launch.Log($"PERF frozen frame={frame}");
            CompositionTarget.Rendering -= OnFrame;
        }
    }
}

/// <summary>
/// The list's rows: a row of cards, or a cluster after every five. Each kind
/// recycles only its own rows, which rebind to the row they show.
/// </summary>
sealed class RowFactory(Look look, int columns, int depth, Post[] posts, ImageSource[] avatars) : ElementFactory
{
    readonly Stack<CardRow> spareCards = new();
    readonly Stack<ClusterRow> spareClusters = new();

    protected override UIElement GetElementCore(Microsoft.UI.Xaml.Controls.ElementFactoryGetArgs args)
    {
        var row = (int)args.Data;
        var block = row / (CardRowsPerCluster + 1);
        var within = row % (CardRowsPerCluster + 1);
        if (within == CardRowsPerCluster)
        {
            var cluster = spareClusters.Count > 0 ? spareClusters.Pop() : new ClusterRow(look, depth);
            cluster.Bind(block);
            return cluster;
        }
        var cards = spareCards.Count > 0 ? spareCards.Pop() : new CardRow(look, columns, posts, avatars);
        cards.Bind((block * CardRowsPerCluster + within) * columns);
        return cards;
    }

    protected override void RecycleElementCore(Microsoft.UI.Xaml.Controls.ElementFactoryRecycleArgs args)
    {
        switch (args.Element)
        {
            case CardRow cards:
                spareCards.Push(cards);
                break;
            case ClusterRow cluster:
                spareClusters.Push(cluster);
                break;
        }
    }
}

/// <summary>A translucent panel stacked over the list. Its elements never change: each frame sets
/// only the panel's translation and tilt and its nested card's tilt, which the compositor applies
/// to their retained drawing.</summary>
sealed class StackedLayer : Border
{
    static readonly Brush LayerBackground = new SolidColorBrush(Look.ColorOf(0xC01E293B));
    static readonly Brush NestedBackground = new SolidColorBrush(Look.ColorOf(0xE6FFFFFF));

    readonly int layer;
    readonly CompositeTransform place = new();
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
        var title = look.Text(13, Look.White, bold: true);
        title.Text = $"Layer {layer + 1}";
        var cells = new StackPanel { Spacing = 3 };
        for (var row = 0; row < rows; row++)
        {
            var line = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 3 };
            for (var column = 0; column < LayerColumns; column++)
            {
                var cell = row * LayerColumns + column;
                var label = look.Text(9, Look.White);
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
        var lines = new StackPanel { Spacing = 2 };
        lines.Children.Add(heading);
        lines.Children.Add(body);
        var nested = Look.Rounded(NestedBackground, 8, new Thickness(8), lines);
        nested.RenderTransformOrigin = new Point(0.5, 0.5);
        nested.RenderTransform = nestedTilt;
        var stack = new StackPanel { Spacing = 8 };
        stack.Children.Add(title);
        stack.Children.Add(cells);
        stack.Children.Add(nested);
        Child = stack;
        // Tilted about its center, then moved.
        RenderTransformOrigin = new Point(0.5, 0.5);
        RenderTransform = place;
        OnFrame(0);
        Clock.Follow(this, OnFrame);
    }

    void OnFrame(int frame)
    {
        var degrees = LayerDegrees(layer, frame);
        place.Rotation = degrees;
        place.TranslateX = LayerX(layer, frame);
        place.TranslateY = LayerY(layer, frame);
        nestedTilt.Angle = -degrees;
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
        bar = new Bar(2 * s) { Width = 20 * s, Height = 4 * s, Fill = Look.Palette[index % Look.Palette.Length] };
        var row = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 4 * s };
        foreach (var element in new FrameworkElement[] { symbol, price, change, bar })
        {
            element.VerticalAlignment = VerticalAlignment.Center;
            row.Children.Add(element);
        }
        Background = Look.White;
        CornerRadius = new CornerRadius(6 * s);
        Padding = new Thickness(6 * s, 3 * s, 6 * s, 3 * s);
        Child = row;
        OnFrame(Clock.Frame);
        Clock.Follow(this, OnFrame);
    }

    void OnFrame(int frame)
    {
        var cents = TickerCents(quote, frame);
        var text = CentsText(cents);
        if (price.Text != text) price.Text = text;
        text = ChangeText(quote, cents);
        if (change.Text != text) change.Text = text;
        change.Foreground = cents >= quote.BaseCents ? Look.Up : Look.Down;
        bar.Share = (cents - quote.BaseCents + quote.SwingCents) / (2.0 * quote.SwingCents);
    }
}

/// <summary>A row of cards, rebound as the list reuses it.</summary>
sealed class CardRow : Grid
{
    readonly CardView[] cards;
    readonly Post[] posts;
    readonly ImageSource[] avatars;

    public CardRow(Look look, int columns, Post[] posts, ImageSource[] avatars)
    {
        this.posts = posts;
        this.avatars = avatars;
        var s = look.S;
        cards = new CardView[columns];
        for (var column = 0; column < columns; column++)
        {
            if (column > 0) ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(8 * s) });
            ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
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

sealed class CardView : Grid
{
    static readonly string[] FooterLabels = ["likes", "replies", "shares"];

    readonly ImageBrush avatar = new() { Stretch = Stretch.UniformToFill };
    readonly TextBlock title;
    readonly Run author = new();
    readonly Run tag = new();
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
        var avatarCircle = new Ellipse
        {
            Width = 32 * s,
            Height = 32 * s,
            Fill = avatar,
            VerticalAlignment = VerticalAlignment.Center,
            Margin = new Thickness(0, 0, 8 * s, 0),
        };
        title = look.Paragraph(13, Look.Ink, 2, bold: true);
        Look.Style(author, bold: true);
        var subtitle = look.Text(11, Look.Muted);
        subtitle.MaxLines = 1;
        subtitle.TextTrimming = TextTrimming.CharacterEllipsis;
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        subtitle.Inlines.Add(new Run { Text = "by " });
        subtitle.Inlines.Add(author);
        subtitle.Inlines.Add(new Run { Text = " · " });
        subtitle.Inlines.Add(tag);
        var headerText = new StackPanel { VerticalAlignment = VerticalAlignment.Center };
        headerText.Children.Add(title);
        headerText.Children.Add(subtitle);
        var header = new Grid();
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        header.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        header.Children.Add(avatarCircle);
        Look.Place(header, headerText, 1);

        body = look.Paragraph(12, Look.Body, 4);

        progress = new Bar(3 * s) { Height = 6 * s, VerticalAlignment = VerticalAlignment.Center };
        percent = look.Text(10, Look.Muted);
        percent.VerticalAlignment = VerticalAlignment.Center;
        percent.Margin = new Thickness(6 * s, 0, 0, 0);
        var progressRow = new Grid();
        progressRow.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        progressRow.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        progressRow.Children.Add(progress);
        Look.Place(progressRow, percent, 1);

        sparkline = new Sparkline((float)(1.5 * s)) { Height = 36 * s };

        var chipFlow = new FlowPanel { Spacing = 4 * s };
        chips = new (Border, TextBlock)[3];
        for (var index = 0; index < chips.Length; index++)
        {
            var text = look.Text(10, Look.Ink);
            var chip = Look.Rounded(null, 10 * s, new Thickness(8 * s, 3 * s, 8 * s, 3 * s), text);
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
            var counter = new StackPanel();
            counter.Children.Add(stats[index]);
            counter.Children.Add(label);
            Look.Place(footer, counter, index * 2);
        }

        var column = new StackPanel { Spacing = 6 * s };
        foreach (var element in new UIElement[] { header, body, progressRow, sparkline, chipFlow, footer })
        {
            column.Children.Add(element);
        }
        var box = new Border
        {
            Background = Look.White,
            BorderBrush = Look.Hairline,
            BorderThickness = new Thickness(1),
            CornerRadius = new CornerRadius(12 * s),
            // Compose draws its border over the padding; a WinUI border insets
            // the content by both, so the padding gives the border's width back.
            Padding = new Thickness(10 * s - 1),
            Shadow = new ThemeShadow(),
            Translation = new Vector3(0, 0, (float)(3 * s)),
            Child = column,
        };
        Children.Add(box);

        var hot = look.Text(9, Look.White, bold: true);
        hot.Text = "HOT";
        badge = Look.Rounded(Look.Palette[0], 8 * s, new Thickness(6 * s, 2 * s, 6 * s, 2 * s), hot);
        badge.HorizontalAlignment = HorizontalAlignment.Right;
        badge.VerticalAlignment = VerticalAlignment.Top;
        badge.Margin = new Thickness(6 * s);
        badge.Opacity = 0.9;
        // A translucent tag tilting with the frame: rotated, never measured again.
        badge.RenderTransformOrigin = new Point(0.5, 0.5);
        badge.RenderTransform = tilt;
        Children.Add(badge);
        Clock.Follow(this, OnFrame);
    }

    public void Bind(Post post, ImageSource image, int card)
    {
        this.card = card;
        avatar.ImageSource = image;
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
        badge.Visibility = card % 5 == 0 ? Visibility.Visible : Visibility.Collapsed;
        OnFrame(Clock.Frame);
    }

    void OnFrame(int frame)
    {
        var permille = ProgressPermille(card, frame);
        var text = $"{permille / 10}%";
        if (percent.Text != text) percent.Text = text;
        progress.Share = permille / 1000.0;
        sparkline.Invalidate();
        if (badge.Visibility == Visibility.Visible) tilt.Angle = BadgeDegrees(card, frame);
    }
}

/// <summary>A card's 48-point line over a fading fill, drawn with Skia from the frame.</summary>
sealed class Sparkline(float strokeWidth) : SKCanvasElement
{
    int card;
    uint argb;
    readonly SKPaint line = new() { IsAntialias = true, Style = SKPaintStyle.Stroke };
    readonly SKPaint fill = new() { IsAntialias = true, Style = SKPaintStyle.Fill };
    readonly SKPath area = new();
    readonly SKPath stroke = new();
    float shaderHeight = -1;

    public void Bind(int card, uint argb)
    {
        this.card = card;
        if (this.argb == argb) return;
        this.argb = argb;
        line.Color = new SKColor(argb);
        line.StrokeWidth = strokeWidth;
        shaderHeight = -1;
    }

    protected override void RenderOverride(SKCanvas canvas, Size size)
    {
        var frame = Clock.Frame;
        var (width, height) = ((float)size.Width, (float)size.Height);
        if (shaderHeight != height)
        {
            shaderHeight = height;
            fill.Shader = SKShader.CreateLinearGradient(new SKPoint(0, 0), new SKPoint(0, height),
                [new SKColor(argb).WithAlpha(0x40), new SKColor(argb).WithAlpha(0)], SKShaderTileMode.Clamp);
        }
        var step = width / (SparkPoints - 1);
        area.Rewind();
        stroke.Rewind();
        area.MoveTo(0, height);
        for (var index = 0; index < SparkPoints; index++)
        {
            var point = new SKPoint(index * step, height * (1 - (float)SparkValue(card, index, frame)));
            area.LineTo(point);
            if (index == 0) stroke.MoveTo(point);
            else stroke.LineTo(point);
        }
        area.LineTo(width, height);
        area.Close();
        canvas.DrawPath(area, fill);
        canvas.DrawPath(stroke, line);
    }
}

/// <summary>
/// `depth` levels nested inside one another, each a row of three labels above
/// the next level.
/// </summary>
sealed class ClusterRow : Border
{
    readonly Level level;

    public ClusterRow(Look look, int depth)
    {
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
                var text = look.Text(9, Look.White);
                text.TextWrapping = TextWrapping.Wrap;
                var border = Look.Rounded(null, 2 * s, new Thickness(0), text);
                border.VerticalAlignment = VerticalAlignment.Top;
                Look.Place(row, border, chip * 2);
                chips[chip] = (border, text);
            }
            var stack = new StackPanel { Spacing = s };
            stack.Children.Add(row);
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
