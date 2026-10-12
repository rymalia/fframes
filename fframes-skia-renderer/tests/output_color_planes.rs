#![cfg(all(target_os = "macos", feature = "metal"))]
//! `EncoderOptions::output_color` on the Skia frame export: the GPU plane shader (every format
//! it covers) and, for comparison, the CPU conversion, read back exactly through rawvideo.
//!
//! Each frame is one saturated colour; its Y, Cb, Cr must be within a few codes of the exact
//! limited-range matrix asked for, and clearly closer to it than to the other matrix.

use fframes::ffmpeg_sys_fframes::AVPixelFormat::{self, *};
use fframes::ffmpeg_sys_fframes::*;
use fframes::{
    AudioMap, Color, Duration, EncoderOptions, FFramesContext, Frame, OutputColor, RenderOptions,
    Svgr, Video,
};
use fframes_skia_renderer::metal::SkiaMetalCtx;
use fframes_skia_renderer::{
    SkiaFFramesRenderer, SkiaFrameExport, SkiaPipelineConcurrencyPolicy, SkiaPipelineConfig,
};

const SIDE: usize = 64;
const COLORS: [[u8; 3]; 4] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]];

#[derive(Debug)]
struct Swatches;

impl Video for Swatches {
    const FPS: usize = 30;
    const WIDTH: usize = SIDE;
    const HEIGHT: usize = SIDE;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(COLORS.len())
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, frame: Frame, _ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let [r, g, b] = COLORS[frame.index];
        let fill = format!("#{r:02x}{g:02x}{b:02x}");
        fframes::svgr!(
            <svg xmlns="http://www.w3.org/2000/svg" width={Self::WIDTH} height={Self::HEIGHT}>
                <rect x="0" y="0" width={Self::WIDTH} height={Self::HEIGHT} fill={fill} />
            </svg>
        )
    }
}

/// `(Kr, Kb)`
fn coefficients(output_color: OutputColor) -> (f64, f64) {
    match output_color {
        OutputColor::Bt601 => (0.299, 0.114),
        OutputColor::Bt709 => (0.2126, 0.0722),
    }
}

fn exact_yuv((kr, kb): (f64, f64), [r, g, b]: [u8; 3]) -> [f64; 3] {
    let (r, g, b) = (f64::from(r) / 255., f64::from(g) / 255., f64::from(b) / 255.);
    let y = kr * r + (1. - kr - kb) * g + kb * b;
    [
        16. + 219. * y,
        128. + 224. * (b - y) / (2. * (1. - kb)),
        128. + 224. * (r - y) / (2. * (1. - kr)),
    ]
}

/// Centre Y, Cb, Cr of a rawvideo packet (planes packed without padding).
fn centre_yuv(format: AVPixelFormat, packet: &[u8]) -> [f64; 3] {
    let (w, h) = (SIDE, SIDE);
    let (x, y) = (w / 2, h / 2);
    let luma = packet[y * w + x];
    let (cb, cr) = match format {
        AV_PIX_FMT_YUV420P | AV_PIX_FMT_YUVA420P => {
            let (cw, ch) = (w / 2, h / 2);
            let at = (y / 2) * cw + x / 2;
            (packet[w * h + at], packet[w * h + cw * ch + at])
        }
        AV_PIX_FMT_YUV422P => {
            let cw = w / 2;
            let at = y * cw + x / 2;
            (packet[w * h + at], packet[w * h + cw * h + at])
        }
        AV_PIX_FMT_YUV444P => (packet[w * h + y * w + x], packet[2 * w * h + y * w + x]),
        AV_PIX_FMT_NV12 | AV_PIX_FMT_NV21 => {
            let at = w * h + (y / 2) * w + (x / 2) * 2;
            let (first, second) = (packet[at], packet[at + 1]);
            if format == AV_PIX_FMT_NV12 { (first, second) } else { (second, first) }
        }
        other => panic!("no reader for {other:?}"),
    };
    [f64::from(luma), f64::from(cb), f64::from(cr)]
}

/// The video packets of `path`, in order.
fn packets(path: &std::path::Path) -> Vec<Vec<u8>> {
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let mut out = Vec::new();
    unsafe {
        let mut input = std::ptr::null_mut();
        assert_eq!(
            avformat_open_input(&raw mut input, name.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()),
            0
        );
        assert!(avformat_find_stream_info(input, std::ptr::null_mut()) >= 0);
        let mut packet = av_packet_alloc();
        while av_read_frame(input, packet) >= 0 {
            if (*packet).stream_index == 0 {
                out.push(std::slice::from_raw_parts((*packet).data, (*packet).size as usize).to_vec());
            }
            av_packet_unref(packet);
        }
        av_packet_free(&raw mut packet);
        avformat_close_input(&raw mut input);
    }
    out
}

#[test]
fn plane_exports_use_the_output_color() {
    let metal = SkiaMetalCtx::new(SIDE, SIDE).expect("a Metal device");
    let formats = [
        AV_PIX_FMT_YUV420P,
        AV_PIX_FMT_YUVA420P,
        AV_PIX_FMT_NV12,
        AV_PIX_FMT_NV21,
        AV_PIX_FMT_YUV422P,
        AV_PIX_FMT_YUV444P,
    ];
    for mode in [SkiaFrameExport::GpuConversion, SkiaFrameExport::CpuConversion] {
        for format in formats {
            for output_color in [OutputColor::Bt601, OutputColor::Bt709] {
                let dir = std::env::temp_dir().join(format!(
                    "fframes-output-color-planes-{}-{mode:?}-{format:?}-{output_color:?}",
                    std::process::id()
                ));
                std::fs::create_dir_all(&dir).unwrap();
                let output = dir.join("out.nut");
                let options = RenderOptions {
                    logger: fframes::fframes_logger::FFramesLoggerVariant::Silent,
                    tmp_files_directory: Some(&dir.join("chunks")),
                    video_encoder_options: EncoderOptions {
                        preferred_encoder: Some("rawvideo"),
                        pixel_format: format,
                        output_color,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let backend = SkiaFFramesRenderer::new_metal(
                    &metal,
                    SkiaPipelineConfig {
                        concurrency_policy: SkiaPipelineConcurrencyPolicy::Concurrency(1),
                        ..Default::default()
                    },
                )
                .unwrap()
                .frame_export(mode);
                fframes::render(&output, &Swatches, backend, &options)
                    .unwrap_or_else(|err| panic!("{mode:?} {format:?} {output_color:?}: {err:?}"));

                let frames = packets(&output);
                assert_eq!(frames.len(), COLORS.len(), "{mode:?} {format:?}: frames");
                let other = match output_color {
                    OutputColor::Bt601 => OutputColor::Bt709,
                    OutputColor::Bt709 => OutputColor::Bt601,
                };
                for (rgb, packet) in COLORS.iter().zip(&frames) {
                    let got = centre_yuv(format, packet);
                    let want = exact_yuv(coefficients(output_color), *rgb);
                    let not = exact_yuv(coefficients(other), *rgb);
                    for (channel, (g, w)) in got.iter().zip(want).enumerate() {
                        assert!(
                            (g - w).abs() <= 2.,
                            "{mode:?} {format:?} {output_color:?} {rgb:?}: channel {channel} is \
                             {g}, exact {w:.2}"
                        );
                    }
                    let distance = |a: [f64; 3], b: [f64; 3]| -> f64 {
                        a.iter().zip(b).map(|(a, b)| (a - b).abs()).sum()
                    };
                    assert!(
                        distance(got, want) + 8. < distance(got, not),
                        "{mode:?} {format:?} {output_color:?} {rgb:?}: {got:?} not clearly \
                         closer to {want:?} than to {not:?}"
                    );
                }
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
}
