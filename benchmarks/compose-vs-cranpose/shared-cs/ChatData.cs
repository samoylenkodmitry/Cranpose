// The chat beside the patchwork, as `perf-data/src/chat.rs` makes it bit for
// bit: an agent's answers arrive from a data thread as tokens, several a
// frame, and the screen shows them as they come. The Avalonia app compiles
// this file.

namespace PerfCompare;

/// <summary>What a block of a turn is.</summary>
public enum ChatKind { User, Heading, Paragraph, Code, Table, Gif, Buttons }

/// <summary>One block of a turn. <c>Text</c> is the text of a user, heading, paragraph or code block;
/// <c>Rows</c> the rows of a table, whose first row is its header; <c>Labels</c> the buttons.</summary>
public sealed record ChatBlock(ChatKind Kind, string Text = "", string[][]? Rows = null, string[]? Labels = null)
{
    public static ChatBlock User(string text) => new(ChatKind.User, text);

    public static ChatBlock Heading(string text) => new(ChatKind.Heading, text);

    public static ChatBlock Paragraph(string text) => new(ChatKind.Paragraph, text);

    public static ChatBlock Code(string text) => new(ChatKind.Code, text);

    public static ChatBlock Table(string[][] rows) => new(ChatKind.Table, Rows: rows);

    public static ChatBlock Gif { get; } = new(ChatKind.Gif);

    public static ChatBlock Buttons(string[] labels) => new(ChatKind.Buttons, Labels: labels);
}

/// <summary>A token: its number from 1, the chat's last block and the tokens of that block that arrived.</summary>
public readonly record struct ChatToken(int Count, int Block, int Shown);

/// <summary>What a stretch of code is, for its color.</summary>
public enum CodeKind { Keyword, Text, Number, Comment }

/// <summary>A colored stretch of code: where it starts and ends, and its kind.</summary>
public readonly record struct CodeSpan(int Start, int End, CodeKind Kind);

public static class ChatData
{
    public const float Width = 320f;
    public const int TokensPerFrame = 4;
    public const int TokenChars = 6;
    public const int GifHold = 2;
    public const float StatusHeight = 28f;
    public const float SpinnerSize = 16f;
    public const float SpinDegrees = 12f;
    public const float ShimmerStep = 6f;
    public const float ShimmerSpan = 120f;
    public const float GifSize = 64f;
    public const float Text = 12f;
    public const float HeadingText = 14f;
    public const float CodeText = 11f;
    public const uint BackgroundArgb = 0xFF14182B;
    public const uint UserArgb = 0xFF2B3A67;
    public const uint PanelArgb = 0xFF0B0E1A;
    public const uint ShimmerDimArgb = 0xFF64748B;
    public const uint ShimmerBrightArgb = 0xFFFFFFFF;

    /// <summary>The color of code of a kind.</summary>
    public static uint CodeArgb(CodeKind kind) => kind switch
    {
        CodeKind.Keyword => 0xFFC084FC,
        CodeKind.Text => 0xFF86EFAC,
        CodeKind.Number => 0xFFFDBA74,
        _ => 0xFF64748B,
    };

    /// <summary>One turn of the chat.</summary>
    public static readonly ChatBlock[] Turn =
        [
      ChatBlock.User("Why does the list stutter when one row changes?"),
      ChatBlock.Heading("What happens in a frame"),
      ChatBlock.Paragraph(
          "The list stutters because a small change starts a large amount of work. The row " +
          "that changed shows a counter, and the counter gets a new value on every frame. " +
          "The row measures its text again, which is right, because the text is new. The " +
          "row then tells its parent that its size may have changed. The parent does not " +
          "check whether the size really changed. It measures and places every one of its " +
          "children again, the ones on the screen and the ones kept ready beyond its edges. " +
          "Each child measures its own text again, although none of that text changed. A " +
          "frame that should cost one row costs the whole list, and the cost grows with " +
          "the number of rows, not with the size of the change."
      ),
      ChatBlock.Paragraph(
          "The profile shows the same thing from the other side. Most of the frame goes to " +
          "text shaping, and nearly all of the shaped strings are equal to the strings of " +
          "the frame before. The cache of shaped text does not help, because the list asks " +
          "for each string at a width that differs by a fraction of a point from the width " +
          "of the last frame, so every lookup misses. Drawing takes a small part of the " +
          "frame and does not change, because the picture is almost the same."
      ),
      ChatBlock.Heading("Three things to change"),
      ChatBlock.Paragraph(
          "- Give the row a fixed height. A row whose size cannot change needs no new " +
          "layout of its parent, so the work stops at the row."
      ),
      ChatBlock.Paragraph(
          "- Round the width that the list passes to its rows to whole points. Equal " +
          "strings then meet equal widths, and the cache of shaped text answers."
      ),
      ChatBlock.Paragraph(
          "- Read the counter in the draw phase where the toolkit allows it. A value " +
          "that changes only what is drawn should not start composition or layout."
      ),
      ChatBlock.Code(
          "// Measure only the row that changed.\n" +
          "fn relayout(rows: &mut [Row], dirty: usize) -> f32 {\n" +
          "    let row = &mut rows[dirty];\n" +
          "    if row.width == 320.0 {\n" +
          "        return row.height;\n" +
          "    }\n" +
          "    row.label = \"measured\";\n" +
          "    row.height = row.measure(320.0);\n" +
          "    row.height\n" +
          "}\n" +
          "\n" +
          "// A row of fixed height ends the pass.\n" +
          "fn place(rows: &mut [Row], first: usize) {\n" +
          "    let mut top = 0.0;\n" +
          "    for row in &mut rows[first..] {\n" +
          "        row.top = top;\n" +
          "        top = top + row.height;\n" +
          "    }\n" +
          "}\n"
      ),
      ChatBlock.Paragraph(
          "With the boundary in place the frame lays out one row and leaves the others " +
          "alone. I ran the list of two thousand rows before and after the change, on the " +
          "same device and at the same scroll position. These are the passes of one frame, " +
          "as the median of three hundred frames:"
      ),
      ChatBlock.Table([
          ["Pass", "Before", "After", "Change"],
          ["Compose", "1.92 ms", "0.31 ms", "-84%"],
          ["Measure", "4.70 ms", "0.22 ms", "-95%"],
          ["Place", "1.18 ms", "0.09 ms", "-92%"],
          ["Draw", "2.46 ms", "2.41 ms", "-2%"],
          ["Frame", "10.26 ms", "3.03 ms", "-70%"],
        ]),
      ChatBlock.Gif,
      ChatBlock.Paragraph(
          "Drawing is now the largest pass, and it did not move. The next step is to keep " +
          "the drawing of the rows that did not change and to draw only the row with the " +
          "counter. That needs a look at how the list records its rows, which I have not " +
          "read yet. The picture above turns while the benchmark runs; it stops when the " +
          "run ends. Tell me whether to apply the three changes, or to show the diff first."
      ),
      ChatBlock.Buttons(["Apply", "Show diff", "Dismiss"]),
      ChatBlock.Paragraph("Waiting for your answer."),
        ];

    static int TypedTokens(string text) => (text.Length + TokenChars - 1) / TokenChars;

    /// <summary>The tokens <c>block</c> takes to appear whole.</summary>
    public static int Tokens(ChatBlock block) => block.Kind switch
    {
        ChatKind.Heading or ChatKind.Paragraph or ChatKind.Code => TypedTokens(block.Text),
        ChatKind.Table => block.Rows!.Length,
        _ => 1,
    };

    /// <summary>What the status line says the agent does while <c>block</c> is the last one.</summary>
    public static string Doing(ChatBlock block) => block.Kind switch
    {
        ChatKind.User => "Reading",
        ChatKind.Heading or ChatKind.Paragraph => "Writing",
        ChatKind.Code => "Writing code",
        ChatKind.Table => "Filling the table",
        ChatKind.Gif => "Drawing",
        _ => "Waiting",
    };

    static readonly int[] Starts = BlockStarts();
    static readonly int TurnTokens = Turn.Sum(Tokens);

    static int[] BlockStarts()
    {
        var starts = new int[Turn.Length];
        var total = 0;
        for (var index = 0; index < Turn.Length; index++)
        {
            starts[index] = total;
            total += Tokens(Turn[index]);
        }
        return starts;
    }

    /// <summary>The chat after <c>count</c> tokens, <c>count</c> at least 1.</summary>
    public static ChatToken TokenAfter(int count)
    {
        var index = Math.Max(count, 1) - 1;
        var turn = index / TurnTokens;
        var within = index % TurnTokens;
        var block = Turn.Length - 1;
        while (Starts[block] > within) block--;
        return new ChatToken(Math.Max(count, 1), turn * Turn.Length + block, within - Starts[block] + 1);
    }

    /// <summary>The turn of block <c>block</c>.</summary>
    public static int TurnOf(int block) => block / Turn.Length;

    /// <summary>The characters of <c>text</c> that <c>shown</c> tokens show; <see cref="int.MaxValue"/> tokens show all of it.</summary>
    public static int ShownChars(string text, int shown) =>
        shown >= text.Length ? text.Length : Math.Min(shown * TokenChars, text.Length);

    /// <summary>The status line's text after token <c>token</c>.</summary>
    public static string Status(ChatToken token) =>
        $"{Doing(Turn[token.Block % Turn.Length])}\u2026 ({token.Count % 1_000_000:D6} tokens)";

    /// <summary>The mark of turn <c>turn</c> before a paragraph or a code block.</summary>
    public static string TurnLabel(int turn) => $"#{turn % 100_000:D5}";

    static readonly string[] Keywords = ["fn", "let", "mut", "if", "for", "in", "return", "usize", "f32", "Row"];

    static bool IsAlpha(char c) => c is >= 'a' and <= 'z' or >= 'A' and <= 'Z';

    static bool IsDigit(char c) => c is >= '0' and <= '9';

    /// <summary>The colored stretches of <c>code</c>, in order.</summary>
    public static List<CodeSpan> CodeSpans(string code)
    {
        var spans = new List<CodeSpan>();
        var at = 0;
        while (at < code.Length)
        {
            var start = at;
            var c = code[at];
            if (c == '/' && at + 1 < code.Length && code[at + 1] == '/')
            {
                var end = code.IndexOf('\n', at);
                at = end < 0 ? code.Length : end;
                spans.Add(new CodeSpan(start, at, CodeKind.Comment));
            }
            else if (c == '"')
            {
                var end = code.IndexOf('"', at + 1);
                at = end < 0 ? code.Length : end + 1;
                spans.Add(new CodeSpan(start, at, CodeKind.Text));
            }
            else if (IsAlpha(c) || c == '_')
            {
                while (at < code.Length && (IsAlpha(code[at]) || IsDigit(code[at]) || code[at] == '_')) at++;
                if (Array.IndexOf(Keywords, code[start..at]) >= 0) spans.Add(new CodeSpan(start, at, CodeKind.Keyword));
            }
            else if (IsDigit(c))
            {
                while (at < code.Length && (IsDigit(code[at]) || code[at] == '.')) at++;
                spans.Add(new CodeSpan(start, at, CodeKind.Number));
            }
            else
            {
                at++;
            }
        }
        return spans;
    }

    /// <summary>The frame of the animated picture that frame <c>frame</c> of the screen shows, of <c>frames</c> frames.</summary>
    public static int GifFrame(int frame, int frames) => frame / GifHold % Math.Max(frames, 1);
}
