// RGBA pixels as a PNG data URI, for the platform's own image view: one
// stored (uncompressed) deflate block per 64 KiB of scanlines.

/** Hermes provides it; React Native's types do not declare it. */
declare function btoa(binary: string): string;

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) {
    c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  return c >>> 0;
});

function crc32(bytes: Uint8Array, start: number, end: number): number {
  let crc = 0xffffffff;
  for (let index = start; index < end; index++) {
    crc = CRC_TABLE[(crc ^ bytes[index]) & 0xff] ^ (crc >>> 8);
  }
  return (crc ^ 0xffffffff) >>> 0;
}

class Writer {
  bytes: number[] = [];

  u8(value: number) {
    this.bytes.push(value & 0xff);
  }

  u16le(value: number) {
    this.u8(value);
    this.u8(value >>> 8);
  }

  u32be(value: number) {
    this.u8(value >>> 24);
    this.u8(value >>> 16);
    this.u8(value >>> 8);
    this.u8(value);
  }

  chunk(type: string, data: number[]) {
    this.u32be(data.length);
    const start = this.bytes.length;
    for (let index = 0; index < 4; index++) {
      this.u8(type.charCodeAt(index));
    }
    data.forEach(byte => this.u8(byte));
    this.u32be(crc32(Uint8Array.from(this.bytes), start, this.bytes.length));
  }
}

function zlibStored(raw: number[]): number[] {
  const out = new Writer();
  out.u8(0x78);
  out.u8(0x01);
  for (let start = 0; start < raw.length || start === 0; start += 0xffff) {
    const block = raw.slice(start, start + 0xffff);
    out.u8(start + 0xffff >= raw.length ? 1 : 0);
    out.u16le(block.length);
    out.u16le(~block.length & 0xffff);
    block.forEach(byte => out.u8(byte));
  }
  let a = 1;
  let b = 0;
  raw.forEach(byte => {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  });
  out.u32be(((b << 16) | a) >>> 0);
  return out.bytes;
}

/** `width` x `height` RGBA pixels as a `data:image/png;base64,...` URI. */
export function pngDataUri(rgba: Uint8Array, width: number, height: number): string {
  const raw: number[] = [];
  for (let y = 0; y < height; y++) {
    raw.push(0);
    for (let at = y * width * 4; at < (y + 1) * width * 4; at++) {
      raw.push(rgba[at]);
    }
  }
  const png = new Writer();
  [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a].forEach(byte => png.u8(byte));
  const header = new Writer();
  header.u32be(width);
  header.u32be(height);
  [8, 6, 0, 0, 0].forEach(byte => header.u8(byte));
  png.chunk('IHDR', header.bytes);
  png.chunk('IDAT', zlibStored(raw));
  png.chunk('IEND', []);
  let binary = '';
  for (let start = 0; start < png.bytes.length; start += 0x8000) {
    binary += String.fromCharCode(...png.bytes.slice(start, start + 0x8000));
  }
  return `data:image/png;base64,${btoa(binary)}`;
}
