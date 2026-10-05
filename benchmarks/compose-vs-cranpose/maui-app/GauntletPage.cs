// The gauntlet in .NET MAUI: the screen `compose-app/.../Gauntlet.kt` and
// `cranpose-app/src/screens/gauntlet.rs` draw, element for element, in MAUI's
// own idiom. The benchmark's README describes it.
//
// Every frame advances a frame index and everything follows from it, never
// from wall time. MAUI's animation ticker advances it once per platform
// frame; the attached views read it and set only what changed.

using Microsoft.Maui.Controls.Shapes;
using Microsoft.Maui.Layouts;
using PerfCompare;
using static PerfCompare.PerfData;

namespace PerfMaui;

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
}

/// <summary>Colors and text the gauntlet's views share at one tier's scale.</summary>
public sealed class Look(float s)
{
    public static readonly Color Ink = Color.FromUint(0xFF111827);
    public static readonly Color Body = Color.FromUint(0xFF374151);
    public static readonly Color Muted = Color.FromUint(0xFF6B7280);
    public static readonly Color Hairline = Color.FromUint(0xFFE5E7EB);
    public static readonly Color Panel = Color.FromUint(0xFFE2E8F0);
    public static readonly Color Up = Color.FromUint(0xFF16A34A);
    public static readonly Color Down = Color.FromUint(0xFFDC2626);
    public static readonly Color[] LevelBackground = [Color.FromUint(0xFFF1F5F9), Color.FromUint(0xFFCBD5E1)];
    public static readonly Color[] Palette = PaletteArgb.Select(Color.FromUint).ToArray();
    public static readonly Color[] ChipBackground = ChipBackgroundArgb.Select(Color.FromUint).ToArray();
    public static readonly Color[] GradientEnd = GradientEndArgb.Select(Color.FromUint).ToArray();

    /// <summary>Android's own line spacing for Roboto is 1.172 em: this puts lines 1.4 em apart.</summary>
    const double ParagraphSpacing = 1.4 / 1.172;

    public float S { get; } = s;

    public Label Text(float size, Color color, bool bold = false) => new()
    {
        FontSize = size * S,
        TextColor = color,
        FontAttributes = bold ? FontAttributes.Bold : FontAttributes.None,
    };

    public Label Paragraph(float size, Color color, int maxLines, bool bold = false)
    {
        var label = Text(size, color, bold);
        label.LineHeight = ParagraphSpacing;
        label.MaxLines = maxLines;
        label.LineBreakMode = LineBreakMode.TailTruncation;
        return label;
    }

    public static Border Rounded(Color background, double radius, Thickness padding, View content) => new()
    {
        Background = background,
        StrokeThickness = 0,
        StrokeShape = new RoundRectangle { CornerRadius = radius },
        Padding = padding,
        Content = content,
    };

}

/// <summary>Children left to right, wrapping, `Gap` apart both ways, as Compose's FlowRow.</summary>
public sealed class FlowLayout(double gap) : Layout
{
    public double Gap { get; } = gap;

    protected override ILayoutManager CreateLayoutManager() => new FlowLayoutManager(this);
}

sealed class FlowLayoutManager(FlowLayout flow) : LayoutManager(flow)
{
    public override Size Measure(double widthConstraint, double heightConstraint)
    {
        var padding = flow.Padding;
        var maxWidth = widthConstraint - padding.HorizontalThickness;
        double x = 0, lineHeight = 0, height = 0, widest = 0;
        foreach (var child in flow)
        {
            var size = child.Measure(maxWidth, double.PositiveInfinity);
            if (x > 0 && x + size.Width > maxWidth)
            {
                height += lineHeight + flow.Gap;
                x = 0;
                lineHeight = 0;
            }
            x += size.Width + flow.Gap;
            widest = Math.Max(widest, x - flow.Gap);
            lineHeight = Math.Max(lineHeight, size.Height);
        }
        var width = double.IsInfinity(widthConstraint) ? widest + padding.HorizontalThickness : widthConstraint;
        return new Size(width, height + lineHeight + padding.VerticalThickness);
    }

    public override Size ArrangeChildren(Rect bounds)
    {
        var padding = flow.Padding;
        var maxWidth = bounds.Width - padding.HorizontalThickness;
        double x = 0, y = 0, lineHeight = 0;
        foreach (var child in flow)
        {
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > maxWidth)
            {
                y += lineHeight + flow.Gap;
                x = 0;
                lineHeight = 0;
            }
            child.Arrange(new Rect(bounds.X + padding.Left + x, bounds.Y + padding.Top + y, size.Width, size.Height));
            x += size.Width + flow.Gap;
            lineHeight = Math.Max(lineHeight, size.Height);
        }
        return bounds.Size;
    }
}

public sealed class GauntletPage : ContentPage
{
    public const string Tag = "PerfCompare";

    /// <summary>Blocks of five card rows and a cluster: no measurement window reaches the end.</summary>
    const int Blocks = 2000;
    const int RowsPerBlock = CardRowsPerCluster + 1;

    /// <summary>How far the list scrolls each frame, in dp.</summary>
    const double ScrollPerFrame = 3;

    readonly int freeze;
    readonly Grid content;
    readonly CollectionView list;
    bool running = true;
#if ANDROID
    readonly ScrollTracker tracker = new();
#endif

    public GauntletPage(int tierIndex, int freeze)
    {
        this.freeze = freeze;
        // The window is fullscreen like the other apps': nothing to inset.
        SafeAreaEdges = new SafeAreaEdges(SafeAreaRegions.None);
        var tier = GauntletTier(tierIndex);
        var look = new Look(tier.Scale);
        var s = tier.Scale;
        var posts = Posts();
        var avatars = Enumerable.Range(0, AvatarCount).Select(index =>
        {
            var png = Png.Encode(AvatarRgba(index), AvatarSize, AvatarSize);
            return ImageSource.FromStream(() => new MemoryStream(png));
        }).ToArray();

        var panel = new FlowLayout(4 * s) { BackgroundColor = Look.Panel, Padding = new Thickness(6 * s) };
        var quotes = Tickers(tier.Tickers);
        for (var index = 0; index < quotes.Length; index++)
        {
            panel.Add(new TickerTile(look, quotes[index], index));
        }

        list = new CollectionView
        {
            ItemsSource = Enumerable.Range(0, Blocks * RowsPerBlock).ToArray(),
            ItemsLayout = new LinearItemsLayout(ItemsLayoutOrientation.Vertical) { ItemSpacing = 8 * s },
            ItemTemplate = new RowTemplates(
                () => new CardRow(look, tier.Columns, posts, avatars),
                () => new ClusterRow(look, tier.Depth)),
            Header = new BoxView { HeightRequest = 8 * s, Color = Colors.Transparent },
        };
#if ANDROID
        list.HandlerChanged += (_, _) =>
        {
            if (list.Handler?.PlatformView is AndroidX.RecyclerView.Widget.RecyclerView recycler)
            {
                recycler.AddOnScrollListener(tracker);
            }
        };
#endif

        content = new Grid
        {
            HorizontalOptions = LayoutOptions.Start,
            RowDefinitions = [new RowDefinition(GridLength.Auto), new RowDefinition(GridLength.Star)],
        };
        content.Add(panel, 0, 0);
        content.Add(list, 0, 1);

        var topBar = new Grid { BackgroundColor = Color.FromUint(0xFF1E2A4A), Padding = new Thickness(16, 0) };
        var title = new Look(1).Text(20, Colors.White, bold: true);
        title.Text = "Gauntlet";
        title.VerticalOptions = LayoutOptions.Center;
        topBar.Add(title);

        var screen = new Grid
        {
            BackgroundColor = Color.FromUint(0xFFEEF0F5),
            RowDefinitions = [new RowDefinition(56), new RowDefinition(GridLength.Star)],
        };
        screen.Add(topBar, 0, 0);
        screen.Add(content, 0, 1);
        Content = screen;

        Loaded += (_, _) => this.Animate("frames", new Animation(_ => Advance()), length: 1000, repeat: () => running);
    }

    void Advance()
    {
        var frame = Clock.Advance();
        // The width is set once per frame: MAUI measures everything below again.
        var density = DeviceDisplay.Current.MainDisplayInfo.Density;
        content.WidthRequest = Math.Round(Width * density * WidthFraction(frame)) / density;
#if ANDROID
        if (list.Handler?.PlatformView is AndroidX.RecyclerView.Widget.RecyclerView recycler)
        {
            // MAUI's CollectionView scrolls to items, not offsets: the platform
            // list scrolls by the frame's distance, catching up any it dropped.
            recycler.ScrollBy(0, (int)Math.Round(frame * ScrollPerFrame * density) - tracker.Scrolled);
        }
#endif
        if (freeze > 0 && frame >= freeze)
        {
            running = false;
            this.AbortAnimation("frames");
#if ANDROID
            Android.Util.Log.Info(Tag, $"PERF frozen frame={frame}");
#endif
        }
    }

#if ANDROID
    sealed class ScrollTracker : AndroidX.RecyclerView.Widget.RecyclerView.OnScrollListener
    {
        public int Scrolled { get; private set; }

        public override void OnScrolled(AndroidX.RecyclerView.Widget.RecyclerView recyclerView, int dx, int dy) =>
            Scrolled += dy;
    }
#endif
}

sealed class RowTemplates(Func<object> cards, Func<object> cluster) : DataTemplateSelector
{
    readonly DataTemplate cardRow = new(cards);
    readonly DataTemplate clusterRow = new(cluster);

    protected override DataTemplate OnSelectTemplate(object item, BindableObject container) =>
        (int)item % (CardRowsPerCluster + 1) == CardRowsPerCluster ? clusterRow : cardRow;
}

/// <summary>Views that change every frame while they are on screen.</summary>
abstract class FrameView : ContentView
{
    protected FrameView()
    {
        Loaded += (_, _) => Clock.Advanced += OnFrame;
        Unloaded += (_, _) => Clock.Advanced -= OnFrame;
    }

    protected abstract void OnFrame(int frame);
}

/// <summary>A rounded track filled to its share of the frame.</summary>
sealed class BarDrawable(float radius, Func<int, float> share) : IDrawable
{
    public Color Fill { get; set; } = Colors.Black;

    public void Draw(ICanvas canvas, RectF rect)
    {
        canvas.FillColor = Look.Hairline;
        canvas.FillRoundedRectangle(rect, radius);
        canvas.FillColor = Fill;
        canvas.FillRoundedRectangle(new RectF(rect.X, rect.Y, rect.Width * share(Clock.Frame), rect.Height), radius);
    }
}

sealed class TickerTile : FrameView
{
    readonly Ticker quote;
    readonly Label price;
    readonly Label change;
    readonly GraphicsView bar;

    public TickerTile(Look look, Ticker quote, int index)
    {
        this.quote = quote;
        var s = look.S;
        var symbol = look.Text(10, Look.Ink, bold: true);
        symbol.Text = quote.Symbol;
        price = look.Text(10, Look.Body);
        change = look.Text(10, Look.Up);
        bar = new GraphicsView
        {
            WidthRequest = 20 * s,
            HeightRequest = 4 * s,
            Drawable = new BarDrawable(2 * s, Share) { Fill = Look.Palette[index % Look.Palette.Length] },
        };
        var row = new HorizontalStackLayout { Spacing = 4 * s };
        foreach (var view in new View[] { symbol, price, change, bar })
        {
            view.VerticalOptions = LayoutOptions.Center;
            row.Add(view);
        }
        Content = Look.Rounded(Colors.White, 6 * s, new Thickness(6 * s, 3 * s), row);
        OnFrame(Clock.Frame);
    }

    float Share(int frame) => Math.Clamp(
        (TickerCents(quote, frame) - quote.BaseCents + quote.SwingCents) / (2f * quote.SwingCents), 0f, 1f);

    protected override void OnFrame(int frame)
    {
        var cents = TickerCents(quote, frame);
        price.Text = CentsText(cents);
        change.Text = ChangeText(quote, cents);
        change.TextColor = cents >= quote.BaseCents ? Look.Up : Look.Down;
        bar.Invalidate();
    }
}

/// <summary>A row of cards, rebound as the list reuses it.</summary>
sealed class CardRow : Grid
{
    readonly CardView[] cards;
    readonly int columns;
    readonly Post[] posts;
    readonly ImageSource[] avatars;

    public CardRow(Look look, int columns, Post[] posts, ImageSource[] avatars)
    {
        this.columns = columns;
        this.posts = posts;
        this.avatars = avatars;
        Padding = new Thickness(8 * look.S, 0);
        ColumnSpacing = 8 * look.S;
        cards = new CardView[columns];
        for (var column = 0; column < columns; column++)
        {
            ColumnDefinitions.Add(new ColumnDefinition(GridLength.Star));
            cards[column] = new CardView(look) { VerticalOptions = LayoutOptions.Start };
            this.Add(cards[column], column, 0);
        }
    }

    protected override void OnBindingContextChanged()
    {
        base.OnBindingContextChanged();
        if (BindingContext is not int row) return;
        var block = row / (CardRowsPerCluster + 1);
        var within = row % (CardRowsPerCluster + 1);
        var first = (block * CardRowsPerCluster + within) * columns;
        for (var column = 0; column < columns; column++)
        {
            var card = first + column;
            cards[column].Bind(posts[card % PostCount], avatars[card % AvatarCount], card);
        }
    }
}

sealed class CardView : FrameView
{
    static readonly string[] FooterLabels = ["likes", "replies", "shares"];

    readonly Look look;
    readonly Image avatar;
    readonly Label title;
    readonly Span author;
    readonly Span tag;
    readonly Label body;
    readonly GraphicsView progress;
    readonly BarDrawable progressFill;
    readonly Label percent;
    readonly GraphicsView sparkline;
    readonly SparklineDrawable sparklineDrawing;
    readonly (Border Chip, Label Text)[] chips;
    readonly Label[] stats;
    readonly Border badge;
    int card;

    public CardView(Look look)
    {
        this.look = look;
        var s = look.S;
        avatar = new Image
        {
            WidthRequest = 32 * s,
            HeightRequest = 32 * s,
            Aspect = Aspect.AspectFill,
            Clip = new EllipseGeometry { Center = new Point(16 * s, 16 * s), RadiusX = 16 * s, RadiusY = 16 * s },
            VerticalOptions = LayoutOptions.Center,
        };
        title = look.Paragraph(13, Look.Ink, 2, bold: true);
        author = new Span { FontAttributes = FontAttributes.Bold };
        tag = new Span();
        var subtitle = look.Text(11, Look.Muted);
        subtitle.MaxLines = 1;
        subtitle.LineBreakMode = LineBreakMode.TailTruncation;
        // `by Ada Lovelace · #rust`: the author bold, the tag in its color.
        subtitle.FormattedText = new FormattedString { Spans = { new Span { Text = "by " }, author, new Span { Text = " · " }, tag } };
        var headerText = new VerticalStackLayout { VerticalOptions = LayoutOptions.Center, Children = { title, subtitle } };
        var header = new Grid
        {
            ColumnSpacing = 8 * s,
            ColumnDefinitions = [new ColumnDefinition(GridLength.Auto), new ColumnDefinition(GridLength.Star)],
        };
        header.Add(avatar, 0, 0);
        header.Add(headerText, 1, 0);

        body = look.Paragraph(12, Look.Body, 4);

        progressFill = new BarDrawable(3 * s, frame => ProgressPermille(card, frame) / 1000f);
        progress = new GraphicsView { HeightRequest = 6 * s, Drawable = progressFill, VerticalOptions = LayoutOptions.Center };
        percent = look.Text(10, Look.Muted);
        percent.VerticalOptions = LayoutOptions.Center;
        var progressRow = new Grid
        {
            ColumnSpacing = 6 * s,
            ColumnDefinitions = [new ColumnDefinition(GridLength.Star), new ColumnDefinition(GridLength.Auto)],
        };
        progressRow.Add(progress, 0, 0);
        progressRow.Add(percent, 1, 0);

        sparklineDrawing = new SparklineDrawable(1.5f * s);
        sparkline = new GraphicsView { HeightRequest = 36 * s, Drawable = sparklineDrawing };

        var chipFlow = new FlowLayout(4 * s);
        chips = new (Border, Label)[3];
        for (var index = 0; index < chips.Length; index++)
        {
            var text = look.Text(10, Look.Ink);
            var chip = Look.Rounded(Colors.White, 10 * s, new Thickness(8 * s, 3 * s), text);
            chipFlow.Add(chip);
            chips[index] = (chip, text);
        }

        // Three counters split by dividers as tall as the tallest counter:
        // the grid's row is as tall as its tallest cell.
        var footer = new Grid
        {
            ColumnDefinitions =
            [
                new ColumnDefinition(GridLength.Star), new ColumnDefinition(1),
                new ColumnDefinition(GridLength.Star), new ColumnDefinition(1),
                new ColumnDefinition(GridLength.Star),
            ],
        };
        stats = new Label[FooterLabels.Length];
        for (var index = 0; index < FooterLabels.Length; index++)
        {
            if (index > 0) footer.Add(new BoxView { Color = Look.Hairline }, index * 2 - 1, 0);
            stats[index] = look.Text(12, Look.Ink, bold: true);
            stats[index].HorizontalOptions = LayoutOptions.Center;
            var label = look.Text(9, Look.Muted);
            label.Text = FooterLabels[index];
            label.HorizontalOptions = LayoutOptions.Center;
            footer.Add(new VerticalStackLayout { Children = { stats[index], label } }, index * 2, 0);
        }

        var column = new VerticalStackLayout
        {
            Spacing = 6 * s,
            Children = { header, body, progressRow, sparkline, chipFlow, footer },
        };
        var shape = new RoundRectangle { CornerRadius = 12 * s };
        var layers = new Grid();
        layers.Add(new Border
        {
            Background = Colors.White,
            Stroke = Look.Hairline,
            StrokeThickness = 1,
            StrokeShape = shape,
            // Compose draws its border over the padding; a MAUI border insets
            // the content by both, so the padding gives the border's width back.
            Padding = new Thickness(10 * s - 1),
            Shadow = new Shadow { Brush = Colors.Black, Offset = new Point(0, s), Radius = 3 * s, Opacity = 0.3f },
            Content = column,
        });

        var hot = look.Text(9, Colors.White, bold: true);
        hot.Text = "HOT";
        badge = Look.Rounded(Look.Palette[0], 8 * s, new Thickness(6 * s, 2 * s), hot);
        badge.HorizontalOptions = LayoutOptions.End;
        badge.VerticalOptions = LayoutOptions.Start;
        badge.Margin = new Thickness(6 * s);
        badge.Opacity = 0.9;
        layers.Add(badge);
        Content = layers;
    }

    public void Bind(Post post, ImageSource image, int card)
    {
        this.card = card;
        avatar.Source = image;
        title.Text = post.Title;
        author.Text = post.Author;
        var (firstTag, tagColor) = post.Tags[0];
        tag.Text = firstTag;
        tag.TextColor = Look.GradientEnd[tagColor];
        body.Text = post.Body;
        progressFill.Fill = Look.Palette[card % Look.Palette.Length];
        sparklineDrawing.Bind(card, Look.Palette[post.Color]);
        for (var index = 0; index < chips.Length; index++)
        {
            var (text, color) = post.Tags[index];
            chips[index].Chip.Background = Look.ChipBackground[color];
            chips[index].Text.Text = text;
            chips[index].Text.TextColor = Look.GradientEnd[color];
        }
        for (var index = 0; index < stats.Length; index++) stats[index].Text = post.Stats[index];
        badge.IsVisible = card % 5 == 0;
        OnFrame(Clock.Frame);
    }

    protected override void OnFrame(int frame)
    {
        percent.Text = $"{ProgressPermille(card, frame) / 10}%";
        progress.Invalidate();
        sparkline.Invalidate();
        // A translucent tag tilting with the frame: rotated, never measured again.
        if (badge.IsVisible) badge.Rotation = BadgeDegrees(card, frame);
    }
}

/// <summary>A card's 48-point line over a fading fill, drawn from the frame.</summary>
sealed class SparklineDrawable(float strokeWidth) : IDrawable
{
    int card;
    Color line = Colors.Black;
    LinearGradientPaint fill = new();

    public void Bind(int card, Color line)
    {
        this.card = card;
        if (this.line == line) return;
        this.line = line;
        fill = new LinearGradientPaint(
            [new PaintGradientStop(0, line.WithAlpha(0.25f)), new PaintGradientStop(1, line.WithAlpha(0))],
            new Point(0, 0), new Point(0, 1));
    }

    public void Draw(ICanvas canvas, RectF rect)
    {
        var frame = Clock.Frame;
        var step = rect.Width / (SparkPoints - 1);
        var area = new PathF(rect.Left, rect.Bottom);
        var stroke = new PathF();
        for (var index = 0; index < SparkPoints; index++)
        {
            var x = rect.Left + index * step;
            var y = rect.Top + rect.Height * (1 - SparkValue(card, index, frame));
            area.LineTo(x, y);
            if (index == 0) stroke.MoveTo(x, y);
            else stroke.LineTo(x, y);
        }
        area.LineTo(rect.Right, rect.Bottom);
        area.Close();
        canvas.SaveState();
        canvas.SetFillPaint(fill, rect);
        canvas.FillPath(area);
        canvas.RestoreState();
        canvas.StrokeColor = line;
        canvas.StrokeSize = strokeWidth;
        canvas.DrawPath(stroke);
    }
}

/// <summary>
/// `depth` levels nested inside one another, each a row of three labels above
/// the next level.
/// </summary>
sealed class ClusterRow : ContentView
{
    readonly Level level;

    public ClusterRow(Look look, int depth)
    {
        Padding = new Thickness(8 * look.S, 0);
        level = new Level(look, depth);
        Content = level.View;
    }

    protected override void OnBindingContextChanged()
    {
        base.OnBindingContextChanged();
        if (BindingContext is int row) level.Bind(row / (CardRowsPerCluster + 1));
    }

    sealed class Level
    {
        readonly int remaining;
        readonly (Border Chip, Label Text)[] chips = new (Border, Label)[3];
        readonly Level? next;

        public Border View { get; }

        public Level(Look look, int remaining)
        {
            this.remaining = remaining;
            var s = look.S;
            var row = new Grid
            {
                ColumnSpacing = 2 * s,
                ColumnDefinitions =
                [
                    new ColumnDefinition(GridLength.Star), new ColumnDefinition(GridLength.Star),
                    new ColumnDefinition(GridLength.Star),
                ],
            };
            for (var chip = 0; chip < chips.Length; chip++)
            {
                var text = look.Text(9, Colors.White);
                var border = Look.Rounded(Colors.Transparent, 2 * s, new Thickness(0), text);
                border.VerticalOptions = LayoutOptions.Start;
                row.Add(border, chip, 0);
                chips[chip] = (border, text);
            }
            var stack = new VerticalStackLayout { Spacing = s, Children = { row } };
            if (remaining > 0)
            {
                next = new Level(look, remaining - 1);
                stack.Add(next.View);
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
