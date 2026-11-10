// The still wall in Avalonia (`../README.md`): a window of tiles that never
// change, under a clock line whose text changes every frame, with a cursor
// outline that rests on another tile every frame. Its data follows
// `perf-data/src/wall.rs` bit for bit.
//
// In Avalonia's idiom the tiles are controls made once on a canvas; the top
// level's animation frame callback sets the clock's text and the cursor's
// translation, the two things a frame changes.

using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Immutable;
// Android's implicit usings bring their own `Orientation`.
using Orientation = Avalonia.Layout.Orientation;

namespace PerfAvalonia;

/// <summary>The wall's tiers and data.</summary>
public static class WallData
{
    /// <summary>The tiles' width and height in points, tier 1 to 20.</summary>
    static readonly (double Width, double Height)[] Tiers =
    [
        (64, 40), (54, 34), (46, 29), (40, 25), (34, 21), (30, 18), (26, 16), (22, 14), (19, 12), (16, 10),
        (14, 9), (12, 8), (10, 7), (8, 6), (7, 5), (6, 4), (5, 4), (5, 3), (4, 3), (3, 3),
    ];

    public const double Gap = 1;
    public const double ClockHeight = 24;
    public const double ClockText = 14;
    public const double Corner = 2;
    public const double CursorWidth = 2;
    public const uint BackgroundArgb = 0xFF0F1220;

    public static (double Width, double Height) Tier(int tier) => Tiers[Math.Clamp(tier, 1, Tiers.Length) - 1];

    /// <summary>The tiles across and down that fill a wall `width` by `height` points.</summary>
    public static (int Columns, int Rows) Grid((double Width, double Height) tier, double width, double height)
    {
        static int Fit(double space, double tile) => Math.Max(1, (int)Math.Floor((space - Gap) / (tile + Gap)));
        return (Fit(width, tier.Width), Fit(height, tier.Height));
    }

    public static string Label(int index) => index.ToString("D5");

    public static string Value(int index) => ((unchecked((uint)index * 2654435761u) >> 22) % 1000).ToString("D3");

    public static int Cursor(int frame, int tiles) => (int)((long)frame * 37 % Math.Max(tiles, 1));

    public static string Clock(int frame) => $"frame {frame:D6}";

    /// <summary>Where tile `index` sits on a wall `columns` tiles across.</summary>
    public static Point Origin(int index, int columns, (double Width, double Height) tier) =>
        new(Gap + index % columns * (tier.Width + Gap), Gap + index / columns * (tier.Height + Gap));
}

/// <summary>A tile: its label over a dot and its value, on a fill the owner sets. Its texts follow
/// `textHeight`, the height of a tile of the wall. <see cref="Bind"/> shows tile `index`, so a list that
/// recycles its rows reuses the controls.</summary>
public sealed class TileView : Border
{
    readonly TextBlock label;
    readonly TextBlock value;
    readonly Border spot;

    public TileView(Look look, double width, double height, double textHeight, double padding)
    {
        label = look.Text(textHeight * 0.30, Brushes.White, bold: true);
        value = look.Text(textHeight * 0.26, Brushes.White);
        var dot = textHeight * 0.22;
        spot = new Border
        {
            Width = dot,
            Height = dot,
            CornerRadius = new CornerRadius(dot / 2),
            VerticalAlignment = VerticalAlignment.Center,
        };
        Width = width;
        Height = height;
        CornerRadius = new CornerRadius(WallData.Corner);
        Padding = new Thickness(padding);
        ClipToBounds = true;
        Child = new StackPanel
        {
            Children =
            {
                label,
                new StackPanel { Orientation = Orientation.Horizontal, Spacing = dot * 0.5, Children = { spot, value } },
            },
        };
    }

    /// <summary>Shows tile `index`'s label, value and dot; a text that did not change is not set again.</summary>
    public void Bind(int index)
    {
        var text = WallData.Label(index);
        if (label.Text != text) label.Text = text;
        text = WallData.Value(index);
        if (value.Text != text) value.Text = text;
        spot.Background = Look.ChipBackground[index / Look.Palette.Length % Look.ChipBackground.Length];
    }
}

public sealed class WallView : Grid
{
    /// <summary>The wall's area in the window every desktop app opens, under the top bar and the clock line.</summary>
    const double WallWidth = 1280;
    const double WallHeight = 820 - 56 - WallData.ClockHeight;

    readonly int freeze;
    readonly (double Width, double Height) tier;
    readonly int columns;
    readonly int tiles;
    readonly TextBlock clock;
    readonly TranslateTransform cursorPlace = new();
    bool firstFrameLogged;

    public WallView(int tierIndex, int freeze)
    {
        this.freeze = freeze;
        tier = WallData.Tier(tierIndex);
        (columns, var rows) = WallData.Grid(tier, WallWidth, WallHeight);
        tiles = columns * rows;
        TextElement.SetFontFamily(this, new FontFamily(DeviceRoboto.Family));
        Background = new ImmutableSolidColorBrush(Color.FromUInt32(WallData.BackgroundArgb));
        RowDefinitions = new RowDefinitions($"56,{WallData.ClockHeight},*");
        Children.Add(Look.TopBar("Still wall"));

        var look = new Look(1);
        clock = look.Text(WallData.ClockText, Brushes.White);
        clock.Margin = new Thickness(8, 0);
        clock.VerticalAlignment = VerticalAlignment.Center;
        SetRow(clock, 1);
        Children.Add(clock);

        var wall = new Canvas { ClipToBounds = true };
        var height = tier.Height;
        for (var index = 0; index < tiles; index++)
        {
            var tile = new TileView(look, tier.Width, height, height, height * 0.06);
            tile.Bind(index);
            tile.Background = Look.Palette[index % Look.Palette.Length];
            var at = WallData.Origin(index, columns, tier);
            Canvas.SetLeft(tile, at.X);
            Canvas.SetTop(tile, at.Y);
            wall.Children.Add(tile);
        }
        wall.Children.Add(new Border
        {
            Width = tier.Width,
            Height = height,
            CornerRadius = new CornerRadius(WallData.Corner),
            BorderBrush = Brushes.White,
            BorderThickness = new Thickness(WallData.CursorWidth),
            RenderTransform = cursorPlace,
        });
        SetRow(wall, 2);
        Children.Add(wall);
        Show(0);
    }

    void Show(int frame)
    {
        clock.Text = WallData.Clock(frame);
        var at = WallData.Origin(WallData.Cursor(frame, tiles), columns, tier);
        cursorPlace.X = at.X;
        cursorPlace.Y = at.Y;
    }

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
        Show(frame);
        if (freeze > 0 && frame >= freeze)
        {
            Launch.Log($"PERF frozen frame={frame}");
            return;
        }
        TopLevel.GetTopLevel(this)?.RequestAnimationFrame(OnFrame);
    }
}
