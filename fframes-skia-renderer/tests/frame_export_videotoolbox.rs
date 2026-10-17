#![cfg(all(target_os = "macos", feature = "metal"))]

use fframes::ffmpeg_sys_fframes::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX;
use fframes::media::FFmpegDecoder;
use fframes::{
    AudioMap, Color, Duration, EncoderOptions, FFramesContext, FFramesRenderBackend, Frame,
    OutputColor, RenderOptions, Svgr, Video, VideoEncoderInfo,
};
use fframes_skia_renderer::metal::SkiaMetalCtx;
use fframes_skia_renderer::{
    SkiaFFramesRenderer, SkiaFrameExport, SkiaPipelineConcurrencyPolicy, SkiaPipelineConfig,
};

const FPS: usize = 60;
// Three 40-frame segments end at fractional seconds and expose truncated MP4 edit lists.
const FRAMES: usize = 120;
const SWITCH_AT: usize = FRAMES / 2;

#[derive(Debug)]
struct TwoColors;

impl Video for TwoColors {
    const FPS: usize = FPS;
    const WIDTH: usize = 1280;
    const HEIGHT: usize = 720;
    const BACKGROUND_COLOR: Color = Color::BLACK;

    fn duration(&self) -> Duration<'_> {
        Duration::Frames(FRAMES)
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, frame: Frame, _ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let fill = if frame.index < SWITCH_AT {
            "#ff0000"
        } else {
            "#0000ff"
        };

        fframes::svgr!(
            <svg xmlns="http://www.w3.org/2000/svg" width={Self::WIDTH} height={Self::HEIGHT}>
                <rect x="0" y="0" width={Self::WIDTH} height={Self::HEIGHT} fill={fill} />
            </svg>
        )
    }
}

fn center_pixel(decoder: &mut FFmpegDecoder, frame: usize) -> [u8; 4] {
    unsafe {
        assert!(
            decoder.decode_up_to(frame as i64).unwrap(),
            "frame {frame} is missing from the rendered video"
        );

        let image = decoder
            .get_raw_frame()
            .convert_last_decoded_frame_into_svg_image(None)
            .unwrap();
        let offset =
            ((image.height as usize / 2) * image.width as usize + image.width as usize / 2) * 4;
        image.data[offset..offset + 4].try_into().unwrap()
    }
}

/// The video stream's matrix tag, as libavformat reads it.
fn matrix_tag(path: &std::path::Path) -> fframes::ffmpeg_sys_fframes::AVColorSpace {
    use fframes::ffmpeg_sys_fframes::*;
    let name = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    unsafe {
        let mut input = std::ptr::null_mut();
        assert_eq!(
            avformat_open_input(&raw mut input, name.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()),
            0
        );
        assert!(avformat_find_stream_info(input, std::ptr::null_mut()) >= 0);
        let matrix = (*(**(*input).streams).codecpar).color_space;
        avformat_close_input(&raw mut input);
        matrix
    }
}

fn assert_color_close(actual: [u8; 4], expected: [u8; 3], frame: usize) {
    // Tight enough to notice the encoder converting RGB with another matrix than the one
    // the stream is tagged with (BT.709 instead of BT.601 moves pure red by 22).
    let close = actual[..3]
        .iter()
        .zip(expected)
        .all(|(a, e)| a.abs_diff(e) <= 12);
    assert!(
        close,
        "frame {frame}: expected ~{expected:?}, decoded {actual:?}"
    );
}

#[test]
fn videotoolbox_encoders_read_the_frames_skia_rendered() {
    let metal = SkiaMetalCtx::new(TwoColors::WIDTH, TwoColors::HEIGHT).expect("a Metal device");
    let mut ran_videotoolbox = false;

    let configurations = [
        (SkiaFrameExport::Auto, 1),
        (SkiaFrameExport::Auto, 3),
        (SkiaFrameExport::GpuConversion, 1),
        (SkiaFrameExport::CpuConversion, 1),
    ];
    // The decoder converts with the stream's tag, so a frame converted with another matrix
    // than the tag says shows up as a colour error (see `assert_color_close`).
    for (output_color, (encoder, (mode, contexts))) in [OutputColor::Bt601, OutputColor::Bt709]
        .into_iter()
        .flat_map(|color| {
            ["h264_videotoolbox", "hevc_videotoolbox"]
                .into_iter()
                .flat_map(|encoder| configurations.map(|config| (encoder, config)))
                .map(move |run| (color, run))
        })
    {
        let dir = std::env::temp_dir().join(format!(
            "fframes-skia-export-{}-{encoder}-{mode:?}-{contexts}-{output_color:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("out.mp4");
        let options = RenderOptions {
            logger: fframes::fframes_logger::FFramesLoggerVariant::Silent,
            tmp_files_directory: Some(&dir.join("chunks")),
            video_encoder_options: EncoderOptions {
                preferred_encoder: Some(encoder),
                bitrate: Some(4_000_000),
                codec_params: Some(&[("allow_sw", "0")]),
                output_color,
                ..Default::default()
            },
            ..Default::default()
        };

        let info = VideoEncoderInfo::for_output(
            &output,
            (
                TwoColors::WIDTH as i32,
                TwoColors::HEIGHT as i32,
                FPS as i32,
            ),
            &options.video_encoder_options,
        )
        .unwrap();
        if info.name() != encoder {
            eprintln!("skipping {encoder}: FFmpeg was built without the `videotoolbox` feature");
            continue;
        }

        let backend = SkiaFFramesRenderer::new_metal(
            &metal,
            SkiaPipelineConfig {
                encoder_threads: 6,
                concurrency_policy: SkiaPipelineConcurrencyPolicy::Concurrency(contexts),
                ..Default::default()
            },
        )
        .unwrap()
        .frame_export(mode);
        let input = backend.negotiate_encoder_input(&info).unwrap();
        if mode == SkiaFrameExport::Auto {
            assert_eq!(
                input.pixel_format, AV_PIX_FMT_VIDEOTOOLBOX,
                "{encoder} did not get hardware frames"
            );
        } else {
            assert!(!input.is_hardware());
        }

        fframes::render(&output, &TwoColors, backend, &options)
            .unwrap_or_else(|err| panic!("{encoder}: {err:?}"));
        let want = match output_color {
            OutputColor::Bt601 => fframes::ffmpeg_sys_fframes::AVColorSpace::AVCOL_SPC_SMPTE170M,
            OutputColor::Bt709 => fframes::ffmpeg_sys_fframes::AVColorSpace::AVCOL_SPC_BT709,
        };
        assert_eq!(matrix_tag(&output), want, "{encoder} {mode:?} {output_color:?}: stream tag");

        let mut decoder = unsafe { FFmpegDecoder::new(&output, FPS, 1) }.unwrap();
        for frame in 0..FRAMES {
            let expected = if frame < SWITCH_AT {
                [255, 0, 0]
            } else {
                [0, 0, 255]
            };
            assert_color_close(center_pixel(&mut decoder, frame), expected, frame);
            ran_videotoolbox = true;
        }
        assert!(!unsafe { decoder.decode_up_to(FRAMES as i64) }.unwrap());
        drop(decoder);

        let _ = std::fs::remove_dir_all(&dir);
    }
    #[cfg(feature = "videotoolbox")]
    assert!(ran_videotoolbox, "VideoToolbox is enabled but no VideoToolbox encoder ran");
    #[cfg(not(feature = "videotoolbox"))]
    if !ran_videotoolbox {
        eprintln!("NOTE: no VideoToolbox encoder ran; build with `--features fframes/videotoolbox`");
    }
}
