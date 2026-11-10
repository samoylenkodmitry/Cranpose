// The chat beside the patchwork in Avalonia (`../../README.md`): an agent's
// answers arrive from a data thread as tokens and the list shows them as
// they come. Its data follows `perf-data/src/chat.rs` bit for bit
// (`../../shared-cs/ChatData.cs`).
//
// In Avalonia's idiom the list is an ItemsControl over a VirtualizingStackPanel
// that holds only the number of blocks. A block's control is made once per
// place in the turn and recycled for the same place in the next turn. A
// token sets the text of the one block that grew: a code block sets the
// length of its colored Runs, a table adds a row it built before. The list
// follows its end from the ScrollViewer's own change event. The spinner turns
// by a render transform and the status text moves its gradient brush.

using System.Collections.Concurrent;
using System.Collections.ObjectModel;
using System.Xml.Linq;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Controls.Primitives;
using Avalonia.Controls.Templates;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Fonts;
using Avalonia.Media.Immutable;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using PerfCompare;
using Path = System.IO.Path;

namespace PerfAvalonia;

/// <summary>The tokens of the chat as they leave the data layer: a thread of its own sends them through a
/// bounded queue and waits while the screen is behind.</summary>
sealed class ChatFeed
{
    readonly BlockingCollection<ChatToken> tokens = new(ChatData.TokensPerFrame * 4);

    public ChatFeed() => new Thread(Run) { IsBackground = true, Name = "chat-data" }.Start();

    void Run()
    {
        var count = 0;
        while (true)
        {
            count += 1;
            tokens.Add(ChatData.TokenAfter(count));
        }
    }

    /// <summary>The next token, when one has arrived. The screen never waits for one.</summary>
    public bool TryTake(out ChatToken token) => tokens.TryTake(out token);
}

/// <summary>The chat's pictures and the monospaced face, read once from the folders `PERF_CHAT` and
/// `PERF_FONTS` name.</summary>
static class ChatAssets
{
    static readonly string Folder = Environment.GetEnvironmentVariable("PERF_CHAT") ?? "chat-assets";

    /// <summary>The frames of the animated picture, decoded once for every block that shows it.</summary>
    public static readonly Lazy<Bitmap[]> Gif = new(() =>
    {
        var (width, height, frames) = ChatGif.Decode(File.ReadAllBytes(Path.Combine(Folder, "spin.gif")));
        return frames.Select(rgba =>
        {
            var bitmap = new WriteableBitmap(
                new PixelSize(width, height), new Vector(96, 96), PixelFormat.Rgba8888, AlphaFormat.Opaque);
            using var buffer = bitmap.Lock();
            for (var y = 0; y < height; y++)
            {
                System.Runtime.InteropServices.Marshal.Copy(rgba, y * width * 4, buffer.Address + y * buffer.RowBytes, width * 4);
            }
            return (Bitmap)bitmap;
        }).ToArray();
    });

    public static readonly FontFamily Mono = new(DeviceMono.Family);

    /// <summary>The spinner of the status line: the paths of `progress.svg` on a canvas of its size.</summary>
    public static Canvas Spinner()
    {
        var svg = XDocument.Load(Path.Combine(Folder, "progress.svg")).Root!;
        var canvas = new Canvas
        {
            Width = double.Parse(svg.Attribute("width")!.Value),
            Height = double.Parse(svg.Attribute("height")!.Value),
        };
        foreach (var path in svg.Elements().Where(element => element.Name.LocalName == "path"))
        {
            canvas.Children.Add(new Avalonia.Controls.Shapes.Path
            {
                Data = Geometry.Parse(path.Attribute("d")!.Value),
                Stroke = new ImmutableSolidColorBrush(Color.Parse(path.Attribute("stroke")!.Value)),
                StrokeThickness = double.Parse(path.Attribute("stroke-width")!.Value),
                StrokeLineCap = path.Attribute("stroke-linecap")?.Value == "round" ? PenLineCap.Round : PenLineCap.Flat,
            });
        }
        return canvas;
    }
}

/// <summary>Lines of the patchwork and the chat lie 1.4 em apart, as the gauntlet's paragraphs do.</summary>
static class TextLeading
{
    public const double Em = 1.4;
}

/// <summary>The monospaced face of the code, from the folder the other faces come from.</summary>
sealed class DeviceMono : FontCollectionBase
{
    public const string Family = "fonts:Mono#DejaVu Sans Mono";
    static readonly Uri Source = new("fonts:Mono");

    public DeviceMono() => TryAddGlyphTypeface(new MemoryStream(Launch.ReadFont("DejaVuSansMono.ttf")), out _);

    public override Uri Key => Source;
}

/// <summary>What the chat shows: its last block and how much of it shows. Every block before it shows whole.</summary>
sealed class ChatState
{
    public int LastBlock { get; set; } = -1;

    public int LastShown { get; set; }

    /// <summary>The tokens of block `index` that show.</summary>
    public int Shown(int index) => index == LastBlock ? LastShown : int.MaxValue;
}

/// <summary>A block of the chat that shows `shown` tokens of itself.</summary>
interface IBlock
{
    void Bind(int index, int shown);
}

public sealed class ChatView : Grid
{
    readonly ChatFeed feed = new();
    readonly ChatState state = new();
    readonly ObservableCollection<int> blocks = [];
    readonly Blocks list;
    readonly ScrollViewer scroller;
    readonly TextBlock status;
    readonly LinearGradientBrush shimmer;
    readonly RotateTransform turn = new();

    public ChatView()
    {
        FontManager.Current.AddFontCollection(new DeviceMono());
        Width = ChatData.Width;
        Background = Brush(ChatData.BackgroundArgb);
        RowDefinitions = new RowDefinitions($"*,{ChatData.StatusHeight}");
        list = new Blocks(state)
        {
            ItemsSource = blocks,
            ItemsPanel = new FuncTemplate<Panel?>(() => new VirtualizingStackPanel()),
            Padding = new Thickness(8, 8, 8, 0),
        };
        scroller = new ScrollViewer
        {
            Content = list,
            VerticalScrollBarVisibility = ScrollBarVisibility.Hidden,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled,
        };
        // A block that grew lengthens the list: the list follows its end.
        scroller.ScrollChanged += (_, e) =>
        {
            if (e.ExtentDelta.Y != 0) scroller.ScrollToEnd();
        };
        Children.Add(scroller);

        var spinner = new Viewbox { Width = ChatData.SpinnerSize, Height = ChatData.SpinnerSize, Child = ChatAssets.Spinner() };
        spinner.RenderTransform = turn;
        shimmer = new LinearGradientBrush
        {
            SpreadMethod = GradientSpreadMethod.Repeat,
            GradientStops =
            [
                new GradientStop(Color.FromUInt32(ChatData.ShimmerDimArgb), 0),
                new GradientStop(Color.FromUInt32(ChatData.ShimmerBrightArgb), 0.5),
                new GradientStop(Color.FromUInt32(ChatData.ShimmerDimArgb), 1),
            ],
        };
        status = new Look(1).Text(ChatData.Text, shimmer);
        status.LineHeight = ChatData.Text * TextLeading.Em;
        var line = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 6,
            Margin = new Thickness(8, 0),
            VerticalAlignment = VerticalAlignment.Center,
            Children = { spinner, status },
        };
        var bar = new Border { Background = Brush(ChatData.PanelArgb), Child = line };
        SetRow(bar, 1);
        Children.Add(bar);
        Clock.Follow(this, Show);
    }

    static IBrush Brush(uint argb) => new ImmutableSolidColorBrush(Color.FromUInt32(argb));

    // Takes the frame's tokens, never waiting for one, and shows the last of them.
    void Show(int frame)
    {
        turn.Angle = frame * ChatData.SpinDegrees;
        var start = frame * ChatData.ShimmerStep % ChatData.ShimmerSpan;
        shimmer.StartPoint = new RelativePoint(start, 0, RelativeUnit.Absolute);
        shimmer.EndPoint = new RelativePoint(start + ChatData.ShimmerSpan, 0, RelativeUnit.Absolute);

        ChatToken? last = null;
        for (var taken = 0; taken < ChatData.TokensPerFrame && feed.TryTake(out var token); taken++) last = token;
        if (last is not { } arrived) return;
        var before = state.LastBlock;
        state.LastBlock = arrived.Block;
        state.LastShown = arrived.Shown;
        status.Text = ChatData.Status(arrived);
        // The block that was last shows whole once another begins; a block of the list not on screen binds
        // when the list makes its control.
        if (before >= 0 && before != arrived.Block && list.ContainerFromIndex(before) is IBlock whole)
        {
            whole.Bind(before, int.MaxValue);
        }
        while (blocks.Count <= arrived.Block) blocks.Add(blocks.Count);
        if (list.ContainerFromIndex(arrived.Block) is IBlock grown) grown.Bind(arrived.Block, arrived.Shown);
    }

    /// <summary>The list: a control for each place of the turn, recycled for that place in the next turn.</summary>
    sealed class Blocks(ChatState state) : ItemsControl
    {
        static readonly object[] Keys = Enumerable.Range(0, ChatData.Turn.Length).Select(place => (object)place).ToArray();

        protected override Type StyleKeyOverride => typeof(ItemsControl);

        protected override bool NeedsContainerOverride(object? item, int index, out object? recycleKey)
        {
            recycleKey = Keys[index % Keys.Length];
            return true;
        }

        protected override Control CreateContainerForItemOverride(object? item, int index, object? recycleKey) =>
            Block(ChatData.Turn[index % ChatData.Turn.Length]);

        protected override void PrepareContainerForItemOverride(Control container, object? item, int index)
        {
            base.PrepareContainerForItemOverride(container, item, index);
            ((IBlock)container).Bind(index, state.Shown(index));
        }

        static Control Block(ChatBlock block) => block.Kind switch
        {
            ChatKind.User => new UserBlock(block.Text),
            ChatKind.Heading => new TypedBlock(block.Text, heading: true),
            ChatKind.Paragraph => new TypedBlock(block.Text, heading: false),
            ChatKind.Code => new CodeBlock(block.Text),
            ChatKind.Table => new TableBlock(block.Rows!),
            ChatKind.Gif => new GifBlock(),
            _ => new ButtonsBlock(block.Labels!),
        };
    }
}

/// <summary>The user's question, whole at once in a bubble at the right.</summary>
sealed class UserBlock : Border, IBlock
{
    public UserBlock(string text)
    {
        var line = new Look(1).Text(ChatData.Text, Brushes.White);
        line.Text = text;
        line.LineHeight = ChatData.Text * TextLeading.Em;
        line.TextWrapping = TextWrapping.Wrap;
        Background = new ImmutableSolidColorBrush(Color.FromUInt32(ChatData.UserArgb));
        CornerRadius = new CornerRadius(10);
        Padding = new Thickness(10, 6);
        HorizontalAlignment = HorizontalAlignment.Right;
        Margin = new Thickness(0, 0, 0, 8);
        Child = line;
    }

    public void Bind(int index, int shown)
    {
    }
}

/// <summary>A paragraph, or a heading in the bold face: the turn's number, then as much of the text as arrived.</summary>
sealed class TypedBlock : TextBlock, IBlock
{
    readonly string text;
    int turn = -1;
    int chars = -1;

    public TypedBlock(string text, bool heading)
    {
        this.text = text;
        FontSize = heading ? ChatData.HeadingText : ChatData.Text;
        FontWeight = heading ? FontWeight.Bold : FontWeight.Normal;
        LineHeight = FontSize * TextLeading.Em;
        Foreground = Brushes.White;
        TextWrapping = TextWrapping.Wrap;
        Margin = new Thickness(0, 0, 0, 8);
    }

    protected override Type StyleKeyOverride => typeof(TextBlock);

    public void Bind(int index, int shown)
    {
        var now = ChatData.ShownChars(text, shown);
        if (turn == ChatData.TurnOf(index) && chars == now) return;
        turn = ChatData.TurnOf(index);
        chars = now;
        Text = $"{ChatData.TurnLabel(turn)} {text.AsSpan(0, now)}";
    }
}

/// <summary>Code in a monospaced face on a panel. Its Runs are colored once; a token sets the length of the
/// Runs it reaches.</summary>
sealed class CodeBlock : Border, IBlock
{
    readonly string code;
    readonly Run head = new() { Foreground = Tint(CodeKind.Comment) };
    readonly List<(Run Run, string Text, int Start)> runs = [];
    int turn = -1;
    int chars;

    public CodeBlock(string code)
    {
        this.code = code;
        var body = new TextBlock
        {
            FontFamily = ChatAssets.Mono,
            FontSize = ChatData.CodeText,
            LineHeight = ChatData.CodeText * TextLeading.Em,
            Foreground = Brushes.White,
            TextWrapping = TextWrapping.Wrap,
        };
        body.Inlines!.Add(head);
        var at = 0;
        foreach (var span in ChatData.CodeSpans(code))
        {
            Add(body, code[at..span.Start], Brushes.White, at);
            Add(body, code[span.Start..span.End], Tint(span.Kind), span.Start);
            at = span.End;
        }
        Add(body, code[at..], Brushes.White, at);
        Background = new ImmutableSolidColorBrush(Color.FromUInt32(ChatData.PanelArgb));
        CornerRadius = new CornerRadius(6);
        Padding = new Thickness(8);
        Margin = new Thickness(0, 0, 0, 8);
        Child = body;
    }

    static IBrush Tint(CodeKind kind) => new ImmutableSolidColorBrush(Color.FromUInt32(ChatData.CodeArgb(kind)));

    void Add(TextBlock body, string text, IBrush color, int start)
    {
        if (text.Length == 0) return;
        var run = new Run { Foreground = color };
        body.Inlines!.Add(run);
        runs.Add((run, text, start));
    }

    public void Bind(int index, int shown)
    {
        if (turn != ChatData.TurnOf(index))
        {
            turn = ChatData.TurnOf(index);
            head.Text = $"// {ChatData.TurnLabel(turn)}\n";
        }
        var now = ChatData.ShownChars(code, shown);
        var low = Math.Min(chars, now);
        var high = Math.Max(chars, now);
        foreach (var (run, text, start) in runs)
        {
            if (start + text.Length <= low || start >= high) continue;
            var visible = Math.Clamp(now - start, 0, text.Length);
            run.Text = visible == text.Length ? text : text[..visible];
        }
        chars = now;
    }
}

/// <summary>A table whose first row is its header. Its rows are built once; a token adds the next one.</summary>
sealed class TableBlock : StackPanel, IBlock
{
    readonly Border[] rows;

    public TableBlock(string[][] cells)
    {
        Margin = new Thickness(0, 0, 0, 8);
        var look = new Look(1);
        var panel = new ImmutableSolidColorBrush(Color.FromUInt32(ChatData.PanelArgb));
        rows = cells.Select((row, index) =>
        {
            var line = new Grid { ColumnDefinitions = new ColumnDefinitions("*,*,*,*") };
            for (var column = 0; column < row.Length; column++)
            {
                var cell = look.Text(ChatData.CodeText, Brushes.White, bold: index == 0);
                cell.Text = row[column];
                cell.LineHeight = ChatData.CodeText * TextLeading.Em;
                Grid.SetColumn(cell, column);
                line.Children.Add(cell);
            }
            return new Border { Background = index == 0 || index % 2 == 1 ? panel : null, Padding = new Thickness(6, 3), Child = line };
        }).ToArray();
    }

    public void Bind(int index, int shown)
    {
        var count = Math.Min(shown, rows.Length);
        while (Children.Count > count) Children.RemoveAt(Children.Count - 1);
        while (Children.Count < count) Children.Add(rows[Children.Count]);
    }
}

/// <summary>The animated picture: its frames were decoded once and a frame sets the picture it shows.</summary>
sealed class GifBlock : Image, IBlock
{
    readonly Bitmap[] frames = ChatAssets.Gif.Value;
    int current = -1;

    public GifBlock()
    {
        Width = ChatData.GifSize;
        Height = ChatData.GifSize;
        HorizontalAlignment = HorizontalAlignment.Left;
        Margin = new Thickness(0, 0, 0, 8);
        Clock.Follow(this, Show);
    }

    public void Bind(int index, int shown) => Show(Clock.Frame);

    void Show(int frame)
    {
        var next = ChatData.GifFrame(frame, frames.Length);
        if (next == current) return;
        current = next;
        Source = frames[next];
    }
}

/// <summary>A row of buttons.</summary>
sealed class ButtonsBlock : StackPanel, IBlock
{
    public ButtonsBlock(string[] labels)
    {
        Orientation = Orientation.Horizontal;
        Spacing = 8;
        Margin = new Thickness(0, 0, 0, 8);
        foreach (var label in labels)
        {
            Children.Add(new Button
            {
                Content = label,
                Background = new ImmutableSolidColorBrush(Color.FromUInt32(ChatData.UserArgb)),
                Foreground = Brushes.White,
                FontSize = ChatData.Text,
                FontWeight = FontWeight.Bold,
                CornerRadius = new CornerRadius(6),
                Padding = new Thickness(10, 6),
                BorderThickness = new Thickness(0),
            });
        }
    }

    public void Bind(int index, int shown)
    {
    }
}
