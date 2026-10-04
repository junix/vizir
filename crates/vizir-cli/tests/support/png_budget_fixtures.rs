//! Tiny deliberately incomplete images: no large source pixel buffer is built.

use png::{BitDepth, ColorType};

pub fn declared_png(
    width: u32,
    height: u32,
    color: ColorType,
    depth: BitDepth,
    header_only: bool,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if color == ColorType::Indexed {
            encoder.set_palette(&[0, 0, 0, 255, 255, 255][..]);
            encoder.set_trns(&[0, 1][..]);
        }
        let mut writer = encoder.write_header().unwrap();
        // A zlib header without pixels. A verifier that gets past the resource
        // checks would allocate its output buffer before detecting this error.
        writer.write_chunk(png::chunk::IDAT, &[0x78, 0x9c]).unwrap();
    }
    if header_only {
        bytes.truncate(33); // PNG signature + complete IHDR (including CRC).
    }
    assert!(bytes.len() < 128);
    bytes
}

pub fn oversized_pngs() -> Vec<Vec<u8>> {
    let mut fixtures = Vec::new();
    for header_only in [true, false] {
        for (width, height, color, depth) in [
            (4096, 4097, ColorType::Rgba, BitDepth::Eight),
            (4097, 4096, ColorType::Rgba, BitDepth::Sixteen),
            (1, 16_777_217, ColorType::Grayscale, BitDepth::One),
            (16_777_217, 1, ColorType::Grayscale, BitDepth::One),
            (65_536, 65_536, ColorType::Indexed, BitDepth::One),
            (65_536, 65_536, ColorType::Indexed, BitDepth::Two),
            (65_536, 65_536, ColorType::Indexed, BitDepth::Four),
            (65_536, 65_536, ColorType::Indexed, BitDepth::Eight),
            (
                2_147_483_647,
                2_147_483_647,
                ColorType::Rgba,
                BitDepth::Sixteen,
            ),
        ] {
            fixtures.push(declared_png(width, height, color, depth, header_only));
        }
    }
    fixtures
}
