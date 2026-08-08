//! A PNG writer in about a hundred lines, because this crate has no
//! dependencies and Windows has no `import`.
//!
//! # Why the output is uncompressed, on purpose
//!
//! A PNG's pixel data is a zlib stream, and zlib permits **stored** blocks —
//! "here are N literal bytes" — which is a complete, standard, everything-reads-it
//! encoding that needs no Huffman coding and no match finder. That is the whole
//! trick: a real deflate is a few hundred more lines to write and to get wrong,
//! and it would buy disk space in `capture/`, which is gitignored and read by
//! `compare` rather than by a browser. A 1280x720 shot lands at about 2.7 MB.
//!
//! The files are ordinary PNGs — `compare -metric AE a.png b.png` does not know
//! or care how they were compressed, which is the property that matters, because
//! comparing two shots is what the harness is *for*.

use std::path::Path;

/// Write `pixels` (RGB, row-major, `w * h * 3` bytes) to `path` as a PNG.
pub fn write_rgb(path: &Path, w: u32, h: u32, pixels: &[u8]) -> std::io::Result<()> {
    assert_eq!(pixels.len(), (w as usize) * (h as usize) * 3);

    let mut png = Vec::with_capacity(pixels.len() + h as usize + 1024);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: 8 bits a channel, colour type 2 (truecolour), no interlace.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);

    // Each row is prefixed by its filter type. Filtering exists to make the
    // bytes more compressible, and nothing here compresses, so every row says
    // "none" and the raw scanline follows.
    let stride = (w as usize) * 3;
    let mut raw = Vec::with_capacity(h as usize * (stride + 1));
    for row in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&pixels[row * stride..(row + 1) * stride]);
    }

    chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

/// Wrap `data` in a zlib stream made entirely of stored deflate blocks.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    // CMF/FLG for deflate with a 32 KiB window; 0x78 0x01 is the pair whose
    // check value works out, and is what "no compression" conventionally emits.
    let mut out = vec![0x78, 0x01];

    // A stored block carries its length in 16 bits, so anything longer is split.
    // The empty input still needs one block, or there is no final-block flag.
    let mut chunks = data.chunks(0xFFFF).peekable();
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(block) = chunks.next() {
        let last = if chunks.peek().is_none() { 1 } else { 0 };
        let len = block.len() as u16;
        out.push(last);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(block);
    }

    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Append a length-tagged, CRC-checked PNG chunk.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);

    let mut crc = crc32(0xFFFF_FFFF, kind);
    crc = crc32(crc, body);
    out.extend_from_slice(&(!crc).to_be_bytes());
}

/// CRC-32 as PNG specifies it, computed a byte at a time.
///
/// No lookup table: a 1280x720 shot is under three megabytes and this runs once
/// per screenshot, so the table would cost more lines than it saves milliseconds.
fn crc32(seed: u32, bytes: &[u8]) -> u32 {
    let mut crc = seed;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry != 0 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    crc
}

/// Adler-32, which is the checksum a zlib stream ends with.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in bytes {
        // 5552 is the most bytes that can be summed before the 32-bit
        // accumulator can overflow, so the modulo is paid once per run of that
        // length instead of once per byte.
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
