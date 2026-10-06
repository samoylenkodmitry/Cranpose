// RGBA pixels as PNG bytes, for the platform's own image view: one stored
// (uncompressed) deflate block per 64 KiB of scanlines.

namespace PerfMaui;

public static class Png
{
    static readonly uint[] CrcTable = MakeCrcTable();

    static uint[] MakeCrcTable()
    {
        var table = new uint[256];
        for (uint n = 0; n < 256; n++)
        {
            var c = n;
            for (var k = 0; k < 8; k++) c = (c & 1) != 0 ? 0xEDB88320u ^ (c >> 1) : c >> 1;
            table[n] = c;
        }
        return table;
    }

    static uint Crc32(byte[] bytes)
    {
        var crc = 0xFFFFFFFFu;
        foreach (var value in bytes) crc = CrcTable[(crc ^ value) & 0xFF] ^ (crc >> 8);
        return crc ^ 0xFFFFFFFFu;
    }

    public static byte[] Encode(byte[] rgba, int width, int height)
    {
        var raw = new byte[height * (width * 4 + 1)];
        for (var y = 0; y < height; y++)
        {
            Buffer.BlockCopy(rgba, y * width * 4, raw, y * (width * 4 + 1) + 1, width * 4);
        }
        using var png = new MemoryStream();
        png.Write([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        var header = new byte[13];
        WriteBigEndian(header, 0, (uint)width);
        WriteBigEndian(header, 4, (uint)height);
        header[8] = 8;
        header[9] = 6;
        Chunk(png, "IHDR", header);
        Chunk(png, "IDAT", ZlibStored(raw));
        Chunk(png, "IEND", []);
        return png.ToArray();
    }

    static byte[] ZlibStored(byte[] raw)
    {
        using var zlib = new MemoryStream();
        zlib.WriteByte(0x78);
        zlib.WriteByte(0x01);
        for (var start = 0; start < raw.Length || start == 0; start += 0xFFFF)
        {
            var length = Math.Min(0xFFFF, raw.Length - start);
            zlib.WriteByte((byte)(start + 0xFFFF >= raw.Length ? 1 : 0));
            zlib.WriteByte((byte)length);
            zlib.WriteByte((byte)(length >> 8));
            zlib.WriteByte((byte)~length);
            zlib.WriteByte((byte)(~length >> 8));
            zlib.Write(raw, start, length);
        }
        uint a = 1, b = 0;
        foreach (var value in raw)
        {
            a = (a + value) % 65521;
            b = (b + a) % 65521;
        }
        var adler = new byte[4];
        WriteBigEndian(adler, 0, b << 16 | a);
        zlib.Write(adler);
        return zlib.ToArray();
    }

    static void Chunk(Stream png, string type, byte[] data)
    {
        var length = new byte[4];
        WriteBigEndian(length, 0, (uint)data.Length);
        png.Write(length);
        var body = new byte[4 + data.Length];
        for (var index = 0; index < 4; index++) body[index] = (byte)type[index];
        data.CopyTo(body, 4);
        png.Write(body);
        var crc = new byte[4];
        WriteBigEndian(crc, 0, Crc32(body));
        png.Write(crc);
    }

    static void WriteBigEndian(byte[] buffer, int offset, uint value)
    {
        buffer[offset] = (byte)(value >> 24);
        buffer[offset + 1] = (byte)(value >> 16);
        buffer[offset + 2] = (byte)(value >> 8);
        buffer[offset + 3] = (byte)value;
    }
}
