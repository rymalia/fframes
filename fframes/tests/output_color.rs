//! `EncoderOptions::output_color`: the RGB to YUV matrix of every CPU conversion path and the
//! colour tags of the encoded stream and of every encoded frame.
//!
//! The matrix tests compare against the exact BT.601 / BT.709 limited-range matrices (computed
//! here from Kr and Kb) and check that saturated colours land clearly closer to the requested
//! matrix than to the other one: a test that both matrices would pass proves nothing.

use fframes::cpu::CpuRenderingBackend;
use fframes::ffmpeg_sys_fframes::AVPixelFormat::{self, *};
use fframes::pix_fmt::{RGB_TO_YUV, RGB_TO_YUV_BT709};
use fframes::{
    AudioMap, Color, Duration, EncoderOptions, FFramesContext, FFramesLoggerVariant, Frame,
    OutputColor, RenderOptions, RgbaFrameConverter, Svgr, Video,
};

/// `(Kr, Kb)`
const BT601: (f64, f64) = (0.299, 0.114);
const BT709: (f64, f64) = (0.2126, 0.0722);

/// Exact limited-range Y, Cb, Cr of an 8-bit RGB colour, in 8-bit codes.
fn exact_yuv((kr, kb): (f64, f64), [r, g, b]: [u8; 3]) -> [f64; 3] {
    let (r, g, b) = (f64::from(r) / 255., f64::from(g) / 255., f64::from(b) / 255.);
    let y = kr * r + (1. - kr - kb) * g + kb * b;
    [
        16. + 219. * y,
        128. + 224. * (b - y) / (2. * (1. - kb)),
        128. + 224. * (r - y) / (2. * (1. - kr)),
    ]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum()
}

/// Saturated colours: the two matrices put them several codes apart (pure red's Y: about
/// 81.5 in BT.601, 62.6 in BT.709).
const DISCRIMINATORS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];

const SIDE: i32 = 16;

/// A `SIDE`x`SIDE` frame of one colour through `RgbaFrameConverter`, read back at the centre
/// as Y, Cb, Cr in 8-bit codes (10-bit formats divided by 4).
fn convert(format: AVPixelFormat, output_color: OutputColor, rgb: [u8; 3]) -> [f64; 3] {
    let mut converter = RgbaFrameConverter::with_color(format, SIDE, SIDE, output_color)
        .unwrap_or_else(|err| panic!("{format:?} converter: {err}"));
    let rgba: Vec<u8> = std::iter::repeat_n([rgb[0], rgb[1], rgb[2], 255], (SIDE * SIDE) as usize)
        .flatten()
        .collect();
    let frame = converter.convert(&rgba).unwrap();
    let raw = unsafe { &*frame.as_ptr() };
    let (x, y) = (SIDE as usize / 2, SIDE as usize / 2);

    let sample = |plane: usize, x: usize, y: usize, ten_bit: bool| -> f64 {
        let row = unsafe { raw.data[plane].add(y * raw.linesize[plane] as usize) };
        if ten_bit {
            let value = unsafe { row.cast::<u16>().add(x).read_unaligned() };
            f64::from(value) / 4.
        } else {
            f64::from(unsafe { *row.add(x) })
        }
    };

    match format {
        AV_PIX_FMT_YUV420P => [sample(0, x, y, false), sample(1, x / 2, y / 2, false), sample(2, x / 2, y / 2, false)],
        AV_PIX_FMT_YUV444P => [sample(0, x, y, false), sample(1, x, y, false), sample(2, x, y, false)],
        AV_PIX_FMT_YUV422P10LE => [sample(0, x, y, true), sample(1, x / 2, y, true), sample(2, x / 2, y, true)],
        AV_PIX_FMT_NV12 => {
            let chroma = unsafe { raw.data[1].add((y / 2) * raw.linesize[1] as usize + (x / 2) * 2) };
            let (cb, cr) = unsafe { (*chroma, *chroma.add(1)) };
            [sample(0, x, y, false), f64::from(cb), f64::from(cr)]
        }
        other => panic!("no reader for {other:?}"),
    }
}

#[test]
fn bt709_weights_match_the_exact_matrix() {
    let (kr, kb) = BT709;
    let kg = 1. - kr - kb;
    let (y, c) = (219. / 255., 224. / 255.);
    let exact = [
        [kr * y, kg * y, kb * y, 16. / 255.],
        [-kr / (2. * (1. - kb)) * c, -kg / (2. * (1. - kb)) * c, 0.5 * c, 128. / 255.],
        [0.5 * c, -kg / (2. * (1. - kr)) * c, -kb / (2. * (1. - kr)) * c, 128. / 255.],
    ];
    for (row, (got, want)) in RGB_TO_YUV_BT709.iter().zip(exact).enumerate() {
        for (col, (got, want)) in got.iter().zip(want).enumerate() {
            assert!(
                (f64::from(*got) - want).abs() < 1e-6,
                "RGB_TO_YUV_BT709[{row}][{col}] = {got}, exact {want}"
            );
        }
    }
    // The BT.601 table the GPU converters always used, unchanged.
    assert_eq!(
        RGB_TO_YUV,
        [
            [66. / 256., 129. / 256., 25. / 256., 16. / 255.],
            [-38. / 256., -74. / 256., 112. / 256., 128. / 255.],
            [112. / 256., -94. / 256., -18. / 256., 128. / 255.],
        ]
    );
}

#[test]
fn converters_use_the_requested_matrix() {
    // (format, tolerance in 8-bit codes against the exact matrix). yuv420p's BT.601 path is
    // the built-in integer converter (66/129/25 weights): within 1.5 codes of exact.
    let formats = [
        (AV_PIX_FMT_YUV420P, 1.5),
        (AV_PIX_FMT_YUV444P, 1.5),
        (AV_PIX_FMT_NV12, 1.5),
        (AV_PIX_FMT_YUV422P10LE, 1.0),
    ];
    for (format, tolerance) in formats {
        for (output_color, wanted, other) in [
            (OutputColor::Bt709, BT709, BT601),
            (OutputColor::Bt601, BT601, BT709),
        ] {
            for rgb in DISCRIMINATORS {
                let got = convert(format, output_color, rgb);
                let (want, not) = (exact_yuv(wanted, rgb), exact_yuv(other, rgb));
                for (channel, (g, w)) in got.iter().zip(want).enumerate() {
                    assert!(
                        (g - w).abs() <= tolerance,
                        "{format:?} {output_color:?} {rgb:?}: channel {channel} is {g}, exact {w:.2}"
                    );
                }
                // Clearly the requested matrix, not the other one.
                assert!(
                    distance(got, want) + 8. < distance(got, not),
                    "{format:?} {output_color:?} {rgb:?}: {got:?} is not clearly closer to \
                     {want:?} than to {not:?}"
                );
            }
        }
    }
}

#[test]
fn rgb_outputs_ignore_the_matrix() {
    for format in [AV_PIX_FMT_RGB24, AV_PIX_FMT_BGRA] {
        let frame = |output_color| {
            let mut converter = RgbaFrameConverter::with_color(format, SIDE, SIDE, output_color).unwrap();
            let rgba: Vec<u8> = (0..SIDE * SIDE * 4).map(|i| (i * 37 % 256) as u8).collect();
            let frame = converter.convert(&rgba).unwrap();
            let raw = unsafe { &*frame.as_ptr() };
            let bytes_per_row = SIDE as usize * if format == AV_PIX_FMT_RGB24 { 3 } else { 4 };
            (0..SIDE as usize)
                .flat_map(|y| unsafe {
                    std::slice::from_raw_parts(raw.data[0].add(y * raw.linesize[0] as usize), bytes_per_row)
                        .to_vec()
                })
                .collect::<Vec<u8>>()
        };
        assert_eq!(frame(OutputColor::Bt601), frame(OutputColor::Bt709), "{format:?}");
    }
}

// --- end to end: a segmented ProRes render ------------------------------------------------------

const FRAMES: usize = 96; // 4 GOPs of the default 24: several segments, joined by the concatenator

#[derive(Debug)]
struct Red;

impl Video for Red {
    const FPS: usize = 30;
    const WIDTH: usize = 64;
    const HEIGHT: usize = 64;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(FRAMES)
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, _frame: Frame, _ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        fframes::svgr!(
            <svg xmlns="http://www.w3.org/2000/svg" width={Self::WIDTH} height={Self::HEIGHT}>
                <rect x="0" y="0" width={Self::WIDTH} height={Self::HEIGHT} fill="#ff0000" />
            </svg>
        )
    }
}

/// `(primaries, transfer, matrix)` of every ProRes frame header in the file, in order.
fn prores_frame_colors(bytes: &[u8]) -> Vec<(u8, u8, u8)> {
    // frame size (4), "icpf", then the frame header: header size (2), version (2), creator
    // (4), width (2), height (2), flags (1), reserved (1), primaries, transfer, matrix
    bytes
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == b"icpf")
        .map(|(i, _)| {
            let h = &bytes[i + 4..i + 4 + 17];
            (h[14], h[15], h[16])
        })
        .collect()
}

fn render_prores(output_color: OutputColor) -> Vec<(u8, u8, u8)> {
    let directory = std::env::temp_dir().join(format!("fframes-output-color-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("out.mov");
    let options = RenderOptions {
        video_encoder_options: EncoderOptions {
            preferred_encoder: Some("prores_ks"),
            pixel_format: AV_PIX_FMT_YUV422P10LE,
            codec_params: Some(&[("profile", "3")]),
            output_color,
            ..Default::default()
        },
        logger: FFramesLoggerVariant::Silent,
        ..Default::default()
    };
    let backend = CpuRenderingBackend { concurrency: 4, ..Default::default() };
    fframes::render(&path, &Red, backend, &options).unwrap_or_else(|err| panic!("render: {err}"));
    let colors = prores_frame_colors(&std::fs::read(&path).unwrap());
    std::fs::remove_dir_all(directory).unwrap();
    colors
}

#[test]
fn every_prores_frame_carries_the_output_color() {
    // ProRes: 1 = bt709, 2 = unspecified, 6 = smpte170m (primaries, transfer, matrix).
    for (output_color, expected) in [(OutputColor::Bt709, (1, 1, 1)), (OutputColor::Bt601, (2, 2, 6))] {
        let colors = render_prores(output_color);
        assert_eq!(colors.len(), FRAMES, "{output_color:?}: frame headers found");
        let wrong: Vec<_> = colors.iter().enumerate().filter(|(_, c)| **c != expected).collect();
        assert!(wrong.is_empty(), "{output_color:?}: frames {wrong:?} differ from {expected:?}");
    }
}
