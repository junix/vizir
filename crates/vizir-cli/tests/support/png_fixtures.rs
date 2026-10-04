//! Exact, generated PNG samples shared by decoder and renderer-routing tests.

use png::{BitDepth, ColorType};

pub struct Fixture {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub transparent_error: Option<&'static str>,
}

fn encode(
    color: ColorType,
    depth: BitDepth,
    pixels: &[u8],
    palette: Option<&[u8]>,
    transparency: Option<&[u8]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some(palette) = palette {
            encoder.set_palette(palette);
        }
        if let Some(transparency) = transparency {
            encoder.set_trns(transparency);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        writer.finish().unwrap();
    }
    bytes
}

pub fn fixtures() -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    let mut add = |name, color, depth, pixels: &[u8], palette, trns, transparent_error| {
        fixtures.push(Fixture {
            name,
            bytes: encode(color, depth, pixels, palette, trns),
            transparent_error,
        });
    };
    let range = Some("VIZ-ARTIFACT-0003");
    let no_alpha = Some("VIZ-ARTIFACT-0002");
    // Color bytes deliberately differ from alpha bytes. For 16-bit inputs,
    // reading every second/fourth byte as an alpha would accept invalid cases.
    for (name, color, depth, pixels, error) in [
        (
            "rgba8 opaque",
            ColorType::Rgba,
            BitDepth::Eight,
            &[0, 0, 0, 255, 0, 0, 0, 255][..],
            range,
        ),
        (
            "rgba8 all-zero",
            ColorType::Rgba,
            BitDepth::Eight,
            &[255, 255, 255, 0, 255, 255, 255, 0],
            range,
        ),
        (
            "rgba8 low-only",
            ColorType::Rgba,
            BitDepth::Eight,
            &[0, 0, 0, 1, 0, 0, 0, 1],
            range,
        ),
        (
            "rgba8 mixed",
            ColorType::Rgba,
            BitDepth::Eight,
            &[255, 255, 255, 0, 0, 0, 0, 1],
            None,
        ),
        (
            "ga8 opaque",
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            &[0, 255, 0, 255],
            range,
        ),
        (
            "ga8 all-zero",
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            &[255, 0, 255, 0],
            range,
        ),
        (
            "ga8 low-only",
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            &[0, 1, 0, 1],
            range,
        ),
        (
            "ga8 mixed",
            ColorType::GrayscaleAlpha,
            BitDepth::Eight,
            &[255, 0, 0, 1],
            None,
        ),
        (
            "rgba16 opaque",
            ColorType::Rgba,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 255, 255],
            range,
        ),
        (
            "rgba16 all-zero",
            ColorType::Rgba,
            BitDepth::Sixteen,
            &[
                255, 255, 255, 255, 255, 255, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0,
            ],
            range,
        ),
        (
            "rgba16 0x0001-only",
            ColorType::Rgba,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1],
            range,
        ),
        (
            "rgba16 mixed 0x0001",
            ColorType::Rgba,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
            None,
        ),
        (
            "rgba16 mixed 0x0100",
            ColorType::Rgba,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0],
            None,
        ),
        (
            "ga16 opaque",
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &[0, 0, 255, 255, 0, 0, 255, 255],
            range,
        ),
        (
            "ga16 all-zero",
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &[255, 255, 0, 0, 255, 255, 0, 0],
            range,
        ),
        (
            "ga16 0x0001-only",
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &[0, 0, 0, 1, 0, 0, 0, 1],
            range,
        ),
        (
            "ga16 mixed 0x0001",
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 0, 1],
            None,
        ),
        (
            "ga16 mixed 0x0100",
            ColorType::GrayscaleAlpha,
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 1, 0],
            None,
        ),
    ] {
        add(name, color, depth, pixels, None, None, error);
    }
    let palette = Some(&[0, 0, 0, 255, 255, 255][..]);
    for (depth, pixels, prefix) in [
        (
            BitDepth::One,
            &[0b0100_0000][..],
            [
                "indexed1 opaque",
                "indexed1 all-zero",
                "indexed1 low-only",
                "indexed1 mixed",
                "indexed1 short tRNS",
                "indexed1 no tRNS",
            ],
        ),
        (
            BitDepth::Two,
            &[0b0001_0000][..],
            [
                "indexed2 opaque",
                "indexed2 all-zero",
                "indexed2 low-only",
                "indexed2 mixed",
                "indexed2 short tRNS",
                "indexed2 no tRNS",
            ],
        ),
        (
            BitDepth::Four,
            &[0b0000_0001][..],
            [
                "indexed4 opaque",
                "indexed4 all-zero",
                "indexed4 low-only",
                "indexed4 mixed",
                "indexed4 short tRNS",
                "indexed4 no tRNS",
            ],
        ),
        (
            BitDepth::Eight,
            &[0, 1][..],
            [
                "indexed8 opaque",
                "indexed8 all-zero",
                "indexed8 low-only",
                "indexed8 mixed",
                "indexed8 short tRNS",
                "indexed8 no tRNS",
            ],
        ),
    ] {
        for (name, trns, error) in [
            (prefix[0], Some(&[255, 255][..]), range),
            (prefix[1], Some(&[0, 0][..]), range),
            (prefix[2], Some(&[1, 1][..]), range),
            (prefix[3], Some(&[0, 1][..]), None),
            (prefix[4], Some(&[0][..]), None), // omitted palette alpha is 255
            (prefix[5], None, no_alpha),
        ] {
            add(
                name,
                ColorType::Indexed,
                depth,
                pixels,
                palette,
                trns,
                error,
            );
        }
    }
    for (name, depth, pixels) in [
        ("gray1", BitDepth::One, &[0b0100_0000][..]),
        ("gray2", BitDepth::Two, &[0b0001_0000][..]),
        ("gray4", BitDepth::Four, &[0b0000_0001][..]),
        ("gray8", BitDepth::Eight, &[0, 1][..]),
        ("gray16", BitDepth::Sixteen, &[0, 0, 0, 1][..]),
    ] {
        add(
            name,
            ColorType::Grayscale,
            depth,
            pixels,
            None,
            None,
            no_alpha,
        );
        add(
            name,
            ColorType::Grayscale,
            depth,
            pixels,
            None,
            Some(&[0, 0]),
            None,
        );
    }
    for (name, depth, pixels) in [
        ("rgb8", BitDepth::Eight, &[0, 0, 0, 0, 0, 1][..]),
        (
            "rgb16",
            BitDepth::Sixteen,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1][..],
        ),
    ] {
        add(name, ColorType::Rgb, depth, pixels, None, None, no_alpha);
        add(
            name,
            ColorType::Rgb,
            depth,
            pixels,
            None,
            Some(&[0, 0, 0, 0, 0, 0]),
            None,
        );
    }
    fixtures
}
