// Generates the dark NSIS installer assets (sidebar + header bitmaps) in pure
// Node, no external dependencies. Run: `node gen_assets.js` from the nsis/ dir.
// (PowerShell alternative with text rendering: gen_assets.ps1)
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Build a 24-bit BMP buffer from a per-pixel callback (top-left origin).
function makeBMP(width, height, pixel) {
  const rowSize = Math.floor((width * 3 + 3) / 4) * 4; // rows padded to 4 bytes
  const dataSize = rowSize * height;
  const fileSize = 54 + dataSize;
  const buf = Buffer.alloc(fileSize);
  buf.write('BM', 0, 'ascii');
  buf.writeUInt32LE(fileSize, 2);
  buf.writeUInt32LE(54, 10); // pixel data offset
  buf.writeUInt32LE(40, 14); // BITMAPINFOHEADER size
  buf.writeInt32LE(width, 18);
  buf.writeInt32LE(height, 22);
  buf.writeUInt16LE(1, 26); // planes
  buf.writeUInt16LE(24, 28); // bpp
  buf.writeUInt32LE(0, 30); // no compression
  buf.writeUInt32LE(dataSize, 34);
  let off = 54;
  const pixelsPerRow = width * 3;
  const pad = rowSize - pixelsPerRow;
  for (let y = height - 1; y >= 0; y--) {
    for (let x = 0; x < width; x++) {
      const [r, g, b] = pixel(x, y);
      buf.writeUInt8(b, off++);
      buf.writeUInt8(g, off++);
      buf.writeUInt8(r, off++);
    }
    for (let p = 0; p < pad; p++) buf.writeUInt8(0, off++);
  }
  return buf;
}

const BG_TOP = [0x15, 0x19, 0x24];
const BG_BOT = [0x0b, 0x0d, 0x12];
const BRAND = [0xe8, 0x9a, 0x4b];

// Sidebar: 164 x 314, vertical gradient + brand accent bar at the bottom.
const sidebar = makeBMP(164, 314, (x, y) => {
  if (y >= 314 - 6) return BRAND;
  const t = y / 314;
  return [
    Math.round(BG_TOP[0] + (BG_BOT[0] - BG_TOP[0]) * t),
    Math.round(BG_TOP[1] + (BG_BOT[1] - BG_TOP[1]) * t),
    Math.round(BG_TOP[2] + (BG_BOT[2] - BG_TOP[2]) * t),
  ];
});
fs.writeFileSync(path.join(__dirname, 'sidebar.bmp'), sidebar);
console.log('wrote sidebar.bmp');

// Header: 150 x 57, solid surface + brand accent bar on the left.
const header = makeBMP(150, 57, (x) => (x < 4 ? BRAND : BG_TOP));
fs.writeFileSync(path.join(__dirname, 'header.bmp'), header);
console.log('wrote header.bmp');
