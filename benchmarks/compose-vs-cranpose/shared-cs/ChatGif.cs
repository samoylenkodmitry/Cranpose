// The animated picture of the patchwork's chat: a GIF decoder, as no .NET
// library or Avalonia decodes the frames of a GIF. The Avalonia app compiles
// this file.

namespace PerfCompare;

public static class ChatGif
{
    /// <summary>The frames of a GIF as RGBA pixels, each the whole picture after the frame was drawn
    /// over the one before.</summary>
    public static (int Width, int Height, byte[][] Frames) Decode(byte[] gif)
    {
        if (gif.Length < 13 || gif[0] != 'G' || gif[1] != 'I' || gif[2] != 'F') throw new InvalidDataException("not a GIF");
        var width = gif[6] | gif[7] << 8;
        var height = gif[8] | gif[9] << 8;
        var at = 13;
        var global = (gif[10] & 0x80) != 0 ? ReadTable(gif, ref at, 2 << (gif[10] & 7)) : null;
        var canvas = new byte[width * height * 4];
        var frames = new List<byte[]>();
        var transparent = -1;
        while (at < gif.Length && gif[at] != 0x3B)
        {
            var block = gif[at++];
            if (block == 0x21)
            {
                var label = gif[at++];
                if (label == 0xF9) transparent = (gif[at + 1] & 1) != 0 ? gif[at + 4] : -1;
                SkipBlocks(gif, ref at);
            }
            else if (block == 0x2C)
            {
                var left = gif[at] | gif[at + 1] << 8;
                var top = gif[at + 2] | gif[at + 3] << 8;
                var frameWidth = gif[at + 4] | gif[at + 5] << 8;
                var frameHeight = gif[at + 6] | gif[at + 7] << 8;
                var flags = gif[at + 8];
                at += 9;
                var table = (flags & 0x80) != 0 ? ReadTable(gif, ref at, 2 << (flags & 7)) : global
                    ?? throw new InvalidDataException("no color table");
                var indices = Lzw(gif, ref at, frameWidth * frameHeight);
                for (var pixel = 0; pixel < indices.Length; pixel++)
                {
                    var index = indices[pixel];
                    if (index == transparent) continue;
                    var to = ((top + pixel / frameWidth) * width + left + pixel % frameWidth) * 4;
                    canvas[to] = table[index * 3];
                    canvas[to + 1] = table[index * 3 + 1];
                    canvas[to + 2] = table[index * 3 + 2];
                    canvas[to + 3] = 0xFF;
                }
                frames.Add((byte[])canvas.Clone());
            }
            else
            {
                throw new InvalidDataException("unknown GIF block");
            }
        }
        return (width, height, frames.ToArray());
    }

    static byte[] ReadTable(byte[] gif, ref int at, int colors)
    {
        var table = gif[at..(at + colors * 3)];
        at += colors * 3;
        return table;
    }

    static void SkipBlocks(byte[] gif, ref int at)
    {
        while (gif[at] != 0) at += gif[at] + 1;
        at++;
    }

    /// <summary>The color indices of one image's data, <c>count</c> of them.</summary>
    static byte[] Lzw(byte[] gif, ref int at, int count)
    {
        var minimum = gif[at++];
        var data = new List<byte>();
        while (gif[at] != 0)
        {
            data.AddRange(gif.AsSpan(at + 1, gif[at]));
            at += gif[at] + 1;
        }
        at++;
        var clear = 1 << minimum;
        var end = clear + 1;
        var prefix = new int[4096];
        var suffix = new byte[4096];
        var stack = new byte[4097];
        var output = new byte[count];
        var written = 0;
        var size = minimum + 1;
        var next = end + 1;
        var previous = -1;
        var first = (byte)0;
        var bits = 0;
        var have = 0;
        var cursor = 0;
        while (written < count)
        {
            while (have < size)
            {
                if (cursor >= data.Count) return output;
                bits |= data[cursor++] << have;
                have += 8;
            }
            var code = bits & ((1 << size) - 1);
            bits >>= size;
            have -= size;
            if (code == clear)
            {
                size = minimum + 1;
                next = end + 1;
                previous = -1;
                continue;
            }
            if (code == end) break;
            var top = 0;
            var current = code;
            if (previous < 0)
            {
                output[written++] = (byte)code;
                previous = code;
                first = (byte)code;
                continue;
            }
            if (code >= next)
            {
                stack[top++] = first;
                current = previous;
            }
            while (current >= clear)
            {
                stack[top++] = suffix[current];
                current = prefix[current];
            }
            first = (byte)current;
            stack[top++] = first;
            if (next < 4096)
            {
                prefix[next] = previous;
                suffix[next] = first;
                next++;
                if (next == 1 << size && size < 12) size++;
            }
            previous = code;
            while (top > 0 && written < count) output[written++] = stack[--top];
        }
        return output;
    }
}
