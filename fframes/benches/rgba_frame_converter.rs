//! `RgbaFrameConverter` at 1080p for each `OutputColor`: BT.601 yuv420p takes the built-in
//! converter, BT.709 yuv420p and every yuv422p10le conversion go through swscale.
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use fframes::ffmpeg_sys_fframes::AVPixelFormat::{AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUV422P10LE};
use fframes::{OutputColor, RgbaFrameConverter};

fn benchmark_rgba_frame_converter(c: &mut Criterion) {
    let mut group = c.benchmark_group("RgbaFrameConverter 1920x1080");
    let (width, height) = (1920, 1080);
    let rgba: Vec<u8> = (0..width * height * 4).map(|_| rand::random::<u8>()).collect();

    for format in [AV_PIX_FMT_YUV420P, AV_PIX_FMT_YUV422P10LE] {
        for output_color in [OutputColor::Bt601, OutputColor::Bt709] {
            let mut converter =
                RgbaFrameConverter::with_color(format, width, height, output_color).unwrap();
            group.bench_function(format!("{format:?}_{output_color:?}"), |b| {
                b.iter(|| converter.convert(black_box(&rgba)).unwrap());
            });
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_rgba_frame_converter);
criterion_main!(benches);
