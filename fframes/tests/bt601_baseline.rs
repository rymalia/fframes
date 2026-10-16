//! The default (BT.601) output keeps the exact pixels fframes produced before
//! `EncoderOptions::output_color` existed: the plane bytes of each default CPU conversion are
//! pinned to hashes taken from upstream `f962cd6` (fframes 1.2.0), where this same test was run
//! unchanged. A difference means the default output changed.

use fframes::RgbaFrameConverter;
use fframes::ffmpeg_sys_fframes::AVPixelFormat::{self, *};

const WIDTH: i32 = 64;
const HEIGHT: i32 = 48;

/// Deterministic RGBA: an LCG, so the input is the same on every run and platform.
fn rgba() -> Vec<u8> {
    let mut state: u32 = 0x2545_f491;
    (0..WIDTH * HEIGHT * 4)
        .map(|i| {
            if i % 4 == 3 {
                return 255;
            }
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

/// FNV-1a, 64 bit: stable across Rust versions, unlike `DefaultHasher`.
fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
}

/// `(plane, bytes per row, rows)` of each plane `format` writes.
fn layout(format: AVPixelFormat) -> Vec<(usize, usize, usize)> {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    match format {
        AV_PIX_FMT_YUV420P => vec![(0, w, h), (1, w / 2, h / 2), (2, w / 2, h / 2)],
        AV_PIX_FMT_YUV422P10LE => vec![(0, w * 2, h), (1, w, h), (2, w, h)],
        AV_PIX_FMT_NV12 => vec![(0, w, h), (1, w, h / 2)],
        other => panic!("no layout for {other:?}"),
    }
}

fn plane_hash(format: AVPixelFormat) -> u64 {
    let mut converter = RgbaFrameConverter::new(format, WIDTH, HEIGHT).unwrap();
    let frame = converter.convert(&rgba()).unwrap();
    let raw = unsafe { &*frame.as_ptr() };
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for (plane, row_bytes, rows) in layout(format) {
        for row in 0..rows {
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    raw.data[plane].add(row * raw.linesize[plane] as usize),
                    row_bytes,
                )
            };
            fnv1a(&mut hash, bytes);
        }
    }
    hash
}

#[test]
fn default_conversions_keep_their_pixels() {
    // From upstream f962cd6, `cargo test --test bt601_baseline` with this file unchanged.
    let expected = [
        (AV_PIX_FMT_YUV420P, 0x3216_45e8_7049_3f96_u64), // built-in converter (NEON on arm64)
        (AV_PIX_FMT_YUV422P10LE, 0x7ed5_00da_d982_2da2_u64), // swscale, the ProRes path
        (AV_PIX_FMT_NV12, 0x9ffa_e7e7_3892_770a_u64),    // swscale
    ];
    let got: Vec<_> = expected
        .iter()
        .map(|(format, _)| (*format, plane_hash(*format)))
        .collect();
    let report: Vec<_> = got
        .iter()
        .map(|(format, hash)| format!("{format:?} {hash:#018x}"))
        .collect();
    assert_eq!(got, expected, "plane hashes: {}", report.join(", "));
}
