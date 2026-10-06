// Deterministic benchmark data: `shared-kotlin/dev/perfcompare/shared/PerfData.kt`
// and `perf-data/src/lib.rs` implement the same generator bit for bit, so every
// app draws identical content. Only what the gauntlet shows is kept. The MAUI
// and Avalonia apps compile this file.

namespace PerfCompare;

public static class PerfData
{
    public const int PostCount = 5000;
    public const int BarCount = 24;
    public const int CardRowsPerCluster = 5;
    public const int AvatarCount = 8;
    public const int AvatarSize = 64;
    public const int SparkPoints = 48;

    static readonly string[] Words =
    [
        "lorem", "ipsum", "dolor", "sit", "amet", "consectetur", "adipiscing", "elit", "sed", "do",
        "eiusmod", "tempor", "incididunt", "ut", "labore", "et", "dolore", "magna", "aliqua", "enim",
        "ad", "minim", "veniam", "quis", "nostrud", "exercitation", "ullamco", "laboris", "nisi",
        "aliquip", "ex", "ea", "commodo", "consequat", "duis", "aute", "irure", "in", "reprehenderit",
        "voluptate", "velit", "esse", "cillum", "fugiat", "nulla", "pariatur",
    ];
    static readonly string[] First =
    [
        "Ada", "Linus", "Grace", "Alan", "Barbara", "Dennis", "Ken", "Margaret", "Edsger", "Donald",
        "Frances", "John", "Radia", "Tim", "Guido", "Bjarne",
    ];
    static readonly string[] Last =
    [
        "Lovelace", "Torvalds", "Hopper", "Turing", "Liskov", "Ritchie", "Thompson", "Hamilton",
        "Dijkstra", "Knuth", "Allen", "McCarthy", "Perlman", "Berners", "Rossum", "Stroustrup",
    ];
    static readonly string[] Tags =
    [
        "#rust", "#kotlin", "#compose", "#android", "#gpu", "#layout", "#text", "#perf", "#wgpu",
        "#skia", "#ui", "#mobile",
    ];

    public static readonly uint[] PaletteArgb =
        [0xFFEF4444, 0xFFF97316, 0xFFEAB308, 0xFF22C55E, 0xFF14B8A6, 0xFF3B82F6, 0xFF8B5CF6, 0xFFEC4899];
    public static readonly uint[] ChipBackgroundArgb =
        [0xFFFEE2E2, 0xFFFFEDD5, 0xFFFEF9C3, 0xFFDCFCE7, 0xFFCCFBF1, 0xFFDBEAFE, 0xFFEDE9FE, 0xFFFCE7F3];
    public static readonly uint[] GradientEndArgb =
        [0xFF7F1D1D, 0xFF7C2D12, 0xFF713F12, 0xFF14532D, 0xFF134E4A, 0xFF1E3A8A, 0xFF4C1D95, 0xFF831843];

    /// <summary>32-bit xorshift, as in the other apps.</summary>
    public sealed class Rng
    {
        uint state;

        public Rng(int seed)
        {
            state = unchecked((uint)seed * 0x9E3779B9u) ^ 0xA5A5A5A5u;
            if (state == 0) state = 1;
        }

        public uint Next()
        {
            var x = state;
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            state = x;
            return x;
        }

        public int Below(int bound) => (int)(Next() % (uint)bound);

        public float Unit() => (Next() >> 8) / 16_777_216f;
    }

    public sealed record Post(int Id, string Author, int Color, string Title, string Body,
        (string Tag, int Color)[] Tags, string[] Stats);

    static string Sentence(Rng rng, int min, int max)
    {
        var count = min + rng.Below(max - min + 1);
        var words = new string[count];
        for (var index = 0; index < count; index++) words[index] = Words[rng.Below(46)];
        var text = string.Join(' ', words);
        return char.ToUpperInvariant(text[0]) + text[1..];
    }

    public static string CompactCount(int value) =>
        value >= 1000 ? $"{value / 1000}.{value % 1000 / 100}k" : value.ToString();

    public static Post MakePost(int index)
    {
        var rng = new Rng(index + 1);
        var first = First[rng.Below(16)];
        var last = Last[rng.Below(16)];
        rng.Below(59); // minutes, in the handle the gauntlet does not show
        rng.Below(1000); // the handle's number
        var color = rng.Below(8);
        var title = Sentence(rng, 4, 8);
        var body = Sentence(rng, 22, 38) + ".";
        for (var bar = 0; bar < BarCount; bar++) rng.Unit();
        var tags = new (string, int)[3];
        for (var tag = 0; tag < 3; tag++)
        {
            var which = rng.Below(Tags.Length);
            tags[tag] = (Tags[which], which % 8);
        }
        var stats = new string[4];
        for (var stat = 0; stat < 4; stat++) stats[stat] = CompactCount(rng.Below(20_000));
        return new Post(index, $"{first} {last}", color, title, body, tags, stats);
    }

    public static Post[] Posts()
    {
        var posts = new Post[PostCount];
        for (var index = 0; index < PostCount; index++) posts[index] = MakePost(index);
        return posts;
    }

    public sealed record Tier(int Columns, float Scale, int Tickers, int Depth);

    static readonly Tier[] Tiers =
    [
        new(1, 1.0f, 8, 6), new(2, 0.85f, 12, 8), new(2, 0.7f, 16, 10), new(3, 0.6f, 20, 12),
        new(3, 0.5f, 28, 14), new(4, 0.45f, 36, 16), new(4, 0.4f, 44, 20), new(5, 0.35f, 56, 24),
        new(6, 0.3f, 72, 28), new(7, 0.27f, 96, 32), new(8, 0.25f, 120, 40), new(10, 0.2f, 160, 48),
        new(12, 0.18f, 200, 56), new(14, 0.16f, 240, 64), new(16, 0.14f, 300, 72), new(20, 0.12f, 400, 80),
    ];

    /// <summary>Tier 1 to 16; anything else is clamped into that range.</summary>
    public static Tier GauntletTier(int tier) => Tiers[Math.Clamp(tier, 1, Tiers.Length) - 1];

    public sealed record Ticker(string Symbol, int BaseCents, int SwingCents, int Step, int Phase);

    public static Ticker[] Tickers(int count)
    {
        var rng = new Rng(31_337);
        var tickers = new Ticker[count];
        for (var index = 0; index < count; index++)
        {
            var length = 3 + rng.Below(2);
            var symbol = new char[length];
            for (var at = 0; at < length; at++) symbol[at] = (char)('A' + rng.Below(26));
            var baseCents = 1_000 + rng.Below(99_000);
            var swingCents = 50 + rng.Below(950);
            var step = 1 + rng.Below(9);
            var phase = rng.Below(2_000);
            tickers[index] = new Ticker(new string(symbol), baseCents, swingCents, step, phase);
        }
        return tickers;
    }

    /// <summary>A triangle wave over the period: 0 at the ends, half the period in the middle.</summary>
    static int Triangle(int value, int period)
    {
        var position = (value % period + period) % period;
        return period / 2 - Math.Abs(position - period / 2);
    }

    /// <summary>The quote's price on the frame, in cents.</summary>
    public static int TickerCents(Ticker ticker, int frame)
    {
        var wave = Triangle(frame * ticker.Step + ticker.Phase, 2_000);
        return ticker.BaseCents + ticker.SwingCents * (wave - 500) / 500;
    }

    static string TwoDecimals(int hundredths)
    {
        var value = Math.Abs(hundredths);
        var fraction = value % 100;
        return $"{value / 100}.{(fraction < 10 ? "0" : "")}{fraction}";
    }

    /// <summary>1234 → 12.34.</summary>
    public static string CentsText(int cents) => (cents < 0 ? "-" : "") + TwoDecimals(cents);

    /// <summary>The change from the base price as a signed percent: +1.25%.</summary>
    public static string ChangeText(Ticker ticker, int cents)
    {
        var basisPoints = (cents - ticker.BaseCents) * 10_000 / ticker.BaseCents;
        return (basisPoints < 0 ? "-" : "+") + TwoDecimals(basisPoints) + "%";
    }

    /// <summary>A card's progress on the frame, in thousandths.</summary>
    public static int ProgressPermille(int card, int frame) => (frame * 3 + card * 37) % 1_000;

    /// <summary>The tilt of a card's badge on the frame, in degrees: -5 to 5.</summary>
    public static float BadgeDegrees(int card, int frame) => Triangle(frame * 2 + card * 30, 40) * 0.5f - 5f;

    /// <summary>The content's share of the screen width on the frame: 0.92 to 1.</summary>
    public static float WidthFraction(int frame) => 0.92f + 0.08f * Triangle(frame * 3, 200) / 100f;

    /// <summary>The sparkline's height at the point, as a fraction of the chart, on the frame.</summary>
    public static float SparkValue(int card, int point, int frame)
    {
        var phase = (frame + card * 7f) * 0.11f;
        return 0.5f + 0.38f * MathF.Sin(point * 0.32f + phase) + 0.08f * MathF.Sin(point * 1.7f + card);
    }

    static readonly byte[][] AvatarColors =
    [
        [0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D], [0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12],
        [0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12], [0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D],
        [0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A], [0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A],
        [0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95], [0xEC, 0x48, 0x99, 0x83, 0x18, 0x43],
    ];

    /// <summary>Avatar pixels as RGBA bytes: a radial gradient crossed by diagonal stripes.</summary>
    public static byte[] AvatarRgba(int index)
    {
        var colors = AvatarColors[index % AvatarCount];
        const int size = AvatarSize;
        var pixels = new byte[size * size * 4];
        var at = 0;
        for (var y = 0; y < size; y++)
        {
            for (var x = 0; x < size; x++)
            {
                var dx = x - size / 2;
                var dy = y - size * 3 / 8;
                var t = Math.Min(255, (dx * dx + dy * dy) * 255 / (40 * 40));
                var stripe = (x + y + index * 3) / 6 % 2 == 0;
                for (var channel = 0; channel < 3; channel++)
                {
                    var value = (colors[channel] * (255 - t) + colors[channel + 3] * t) / 255;
                    if (stripe) value += (255 - value) / 6;
                    pixels[at++] = (byte)value;
                }
                pixels[at++] = 0xFF;
            }
        }
        return pixels;
    }
}
