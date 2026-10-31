//! Argument checks plus end-to-end runs against the real ffmpeg on PATH.

use std::path::Path;

use super::graph::{self, Graph, Target};
use super::preview::preview_args;
use super::*;
use crate::media::{MediaInfo, MediaInfos};
use crate::model::{Clip, ClipId, Content, Project, Span, Track, Transform};
use crate::test_support::{ffmpeg, fixtures, media_info, output_path, run_ok, wait_until};

pub struct Timeline {
    pub project: Project,
    pub infos: MediaInfos,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Color {
    Red,
    Blue,
    Black,
    Other([u8; 3]),
}

#[test]
fn export_args_encode_with_project_settings() {
    let mut tl = Timeline::new(1280, 720, 60.0);
    tl.project.settings.video_codec = "libx265".into();
    tl.project.settings.video_bitrate_kbps = 8000;
    tl.project.settings.audio_bitrate_kbps = 128;
    let args = |output: &str| {
        export_args(
            &tl.project,
            &tl.infos,
            Span::new(0.0, 1.0),
            Path::new(output),
        )
    };
    let args_mp4 = args("out.mp4");
    let tail = args_mp4
        .iter()
        .skip_while(|a| *a != "-map")
        .map(String::as_str);
    assert_eq!(
        tail.collect::<Vec<_>>(),
        [
            "-map",
            "[v]",
            "-map",
            "[a]",
            "-c:v",
            "libx265",
            "-b:v",
            "8000k",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-r",
            "60",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            "-y",
            "out.mp4",
        ]
    );
    assert!(args("a.MOV").contains(&"+faststart".to_owned()));
    assert!(!args("a.mkv").contains(&"+faststart".to_owned()));
}

#[test]
fn preview_renders_one_downscaled_frame_containing_time() {
    let tl = Timeline::new(1920, 1080, 30.0);
    let args = preview_args(&tl.project, &tl.infos, 1.05);
    let graph = &args[args.iter().position(|a| a == "-filter_complex").unwrap() + 1];
    assert!(
        graph.starts_with(
            "color=c=black:s=960x540:r=30,trim=end_frame=1,setpts=PTS+round(1.033333/TB)[base];"
        ),
        "{graph}"
    );
    assert!(
        args.ends_with(
            &["-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "-"].map(String::from)
        )
    );
}

#[test]
fn preview_inputs_read_past_the_frame() {
    let mut tl = Timeline::new(640, 360, 30.0);
    tl.clip("a.mp4", Track::Video(0), 0.0, 10.0);
    let args = preview_args(&tl.project, &tl.infos, 2.0);
    assert_eq!(
        args[2..8],
        ["-ss", "2.000000", "-t", "1.033333", "-i", "a.mp4"]
    );
}

#[test]
fn export_renders_overlapping_clips_in_subrange() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip(&fx.testsrc, Track::Video(0), 0.0, 3.0);
    let second = tl.clip(&fx.testsrc, Track::Video(0), 2.0, 3.0);
    tl.edit(second).audio.mute_regions = vec![Span::new(0.5, 1.0)];
    let output = output_path("overlapping.mp4");
    assert_eq!(
        export(&tl, Span::new(1.0, 4.5), &output),
        ExportStatus::Finished
    );
    let streams = probe_streams(&output);
    assert_eq!(streams.len(), 2, "{streams:?}");
    for stream in &streams {
        let duration = stream.duration.unwrap();
        assert!((duration - 3.5).abs() < 0.05, "{stream:?}");
    }
    assert_eq!(streams[0].kind, "video");
    assert_eq!(streams[0].frames, Some(105.0));
    std::fs::remove_file(output).unwrap();
}

#[test]
fn export_of_empty_timeline_succeeds() {
    let tl = Timeline::new(320, 180, 30.0);
    let output = output_path("empty.mkv");
    assert_eq!(
        export(&tl, Span::new(0.0, 0.5), &output),
        ExportStatus::Finished
    );
    let streams = probe_streams(&output);
    assert_eq!(streams.len(), 2, "{streams:?}");
    std::fs::remove_file(output).unwrap();
}

#[test]
fn export_places_and_mutes_audio() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.project.settings.audio_codec = "pcm_s16le".into();
    let clip = tl.clip(&fx.red_16x9, Track::Video(0), 1.0, 2.0);
    tl.edit(clip).audio.mute_regions = vec![Span::new(0.5, 1.0)];
    let output = output_path("audio.mkv");
    assert_eq!(
        export(&tl, Span::new(0.0, 3.0), &output),
        ExportStatus::Finished
    );
    let samples = audio_samples(&output);
    let peak = |start: f64, end: f64| {
        let index = |t: f64| (t * f64::from(AUDIO_CHECK_RATE)) as usize;
        let window = &samples[index(start)..index(end)];
        window.iter().map(|s| s.unsigned_abs()).max().unwrap()
    };
    assert_eq!(peak(0.0, 0.95), 0, "clip audio is delayed to its start");
    assert!(peak(1.05, 1.45) > 1000);
    assert_eq!(peak(1.55, 1.95), 0, "mute region is in source time");
    assert!(peak(2.05, 2.95) > 1000);
    std::fs::remove_file(output).unwrap();
}

#[test]
fn export_failure_reports_ffmpeg_error() {
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip("definitely-missing.mp4", Track::Video(0), 0.0, 1.0);
    let output = output_path("missing.mp4");
    let status = export(&tl, Span::new(0.0, 1.0), &output);
    let ExportStatus::Failed(message) = status else {
        panic!("{status:?}");
    };
    assert!(message.contains("definitely-missing.mp4"), "{message}");
}

#[test]
fn export_can_be_cancelled() {
    let output = output_path("cancelled.mkv");
    let args = [
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=1280x720:rate=30:duration=600",
        "-c:v",
        "libx264",
        "-progress",
        "pipe:1",
        "-nostats",
        "-y",
    ]
    .map(String::from)
    .into_iter()
    .chain([output.to_string_lossy().into_owned()])
    .collect();
    let mut job = ExportJob::start(&ffmpeg(), args, 600.0, &output).unwrap();
    wait_until(10, || {
        job.status() != (ExportStatus::Running { progress: 0.0 })
    });
    assert!(
        matches!(job.status(), ExportStatus::Running { progress } if progress > 0.0 && progress < 1.0)
    );
    job.cancel();
    assert_eq!(job.status(), ExportStatus::Cancelled);
    wait_until(5, || !output.exists());
}

#[test]
fn preview_composites_tracks_and_cuts() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip(&fx.red_16x9, Track::Video(0), 0.0, 1.01);
    // First source frame after the seek lands mid-slot; the slot must still show it, not black.
    let blue = tl.clip(&fx.blue_4x3, Track::Video(0), 1.01, 1.0);
    tl.edit(blue).source_in = 0.02;
    tl.clip(&fx.blue_4x3, Track::Video(1), 3.0, 1.0);
    tl.clip(&fx.red_16x9, Track::Video(0), 2.5, 2.0);
    let pixel = |time: f64, x: u32| {
        let image = render_preview(&tl, time, process::MAX_INLINE_COMMAND_LINE);
        assert_eq!(image.dimensions(), (320, 180));
        classify(image.get_pixel(x, 90).0)
    };
    assert_eq!(pixel(0.0, 160), Color::Red);
    assert_eq!(pixel(29.0 / 30.0, 160), Color::Red);
    assert_eq!(pixel(1.0, 160), Color::Blue);
    assert_eq!(pixel(2.2, 160), Color::Black);
    assert_eq!(pixel(3.5, 160), Color::Blue);
    // 4:3 source on 16:9 canvas: its pillarbox is transparent, the track below shows through.
    assert_eq!(pixel(3.5, 5), Color::Red);
    assert_eq!(pixel(1.5, 5), Color::Black);
}

#[test]
fn preview_works_with_graph_in_file() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip(&fx.red_16x9, Track::Video(0), 0.0, 1.0);
    let image = render_preview(&tl, 0.5, 0);
    assert_eq!(classify(image.get_pixel(160, 90).0), Color::Red);
}

#[test]
fn previewer_delivers_texture_and_skips_repeated_requests() {
    let fx = fixtures();
    let mut tl = Timeline::new(1280, 720, 30.0);
    tl.clip(&fx.testsrc, Track::Video(0), 0.0, 3.0);
    let ffmpeg = ffmpeg();
    let mut previewer = Previewer::new(egui::Context::default());
    previewer.request(&ffmpeg, &tl.project, &tl.infos, 1.0);
    wait_until(10, || {
        previewer.poll();
        previewer.texture().is_some() || previewer.error().is_some()
    });
    assert_eq!(previewer.error(), None);
    assert_eq!(previewer.texture().unwrap().size(), [960, 540]);
    wait_until(10, || !previewer.is_busy());
    previewer.request(&ffmpeg, &tl.project, &tl.infos, 1.0);
    assert!(!previewer.is_busy());
}

#[test]
fn previewer_reports_errors() {
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip("definitely-missing.mp4", Track::Video(0), 0.0, 1.0);
    let mut previewer = Previewer::new(egui::Context::default());
    previewer.request(&ffmpeg(), &tl.project, &tl.infos, 0.5);
    wait_until(10, || {
        previewer.poll();
        previewer.error().is_some()
    });
    assert!(
        previewer
            .error()
            .unwrap()
            .contains("definitely-missing.mp4")
    );
    assert!(previewer.texture().is_none());
}

impl Timeline {
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        let mut project = Project::default();
        project.settings.width = width;
        project.settings.height = height;
        project.settings.fps = fps;
        Self {
            project,
            infos: MediaInfos::new(),
        }
    }

    /// Source with video and audio.
    pub fn clip(
        &mut self,
        path: impl AsRef<Path>,
        track: Track,
        start: f64,
        duration: f64,
    ) -> ClipId {
        self.clip_with(path, media_info(true, true), track, start, duration)
    }

    pub fn clip_with(
        &mut self,
        path: impl AsRef<Path>,
        info: MediaInfo,
        track: Track,
        start: f64,
        duration: f64,
    ) -> ClipId {
        let source = self.project.add_source(path.as_ref().to_path_buf());
        self.infos.insert(source, info);
        let id = self.project.append_clip(source, track, duration);
        self.project.move_clip(id, start, track);
        id
    }

    pub fn edit(&mut self, id: ClipId) -> &mut Clip {
        self.project.clip_mut(id).unwrap()
    }

    /// Graph of `start..end` at the project size, with audio.
    pub fn build(&self, start: f64, end: f64) -> Graph {
        let settings = &self.project.settings;
        self.build_sized(start, end, settings.width, settings.height)
    }

    pub fn build_sized(&self, start: f64, end: f64, width: u32, height: u32) -> Graph {
        graph::build(
            &self.project,
            &self.infos,
            &Target {
                range: Span { start, end },
                width,
                height,
                audio: true,
                input_lookahead: 0.0,
            },
        )
    }
}

/// Runs the export to the end.
fn export(tl: &Timeline, range: Span, output: &Path) -> ExportStatus {
    let args = export_args(&tl.project, &tl.infos, range, output);
    let job = ExportJob::start(&ffmpeg(), args, range.len(), output).unwrap();
    wait_until(30, || !matches!(job.status(), ExportStatus::Running { .. }));
    job.status()
}

#[derive(Debug)]
struct Stream {
    kind: String,
    duration: Option<f64>,
    frames: Option<f64>,
}

fn probe_streams(path: &Path) -> Vec<Stream> {
    let output = run_ok(
        ffmpeg()
            .ffprobe()
            .args([
                "-v",
                "error",
                "-count_frames",
                "-of",
                "json",
                "-show_entries",
            ])
            .arg("stream=codec_type,duration,nb_read_frames")
            .arg(path),
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    json["streams"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stream| {
            let number = |key: &str| stream[key].as_str().and_then(|v| v.parse().ok());
            Stream {
                kind: stream["codec_type"].as_str().unwrap().to_owned(),
                duration: number("duration"),
                frames: number("nb_read_frames"),
            }
        })
        .collect()
}

const AUDIO_CHECK_RATE: u32 = 8000;

/// Mono samples of the first audio stream at `AUDIO_CHECK_RATE`.
fn audio_samples(path: &Path) -> Vec<i16> {
    let output = run_ok(
        ffmpeg()
            .ffmpeg()
            .args(["-loglevel", "error", "-i"])
            .arg(path)
            .args(["-map", "0:a:0", "-ac", "1", "-ar"])
            .arg(AUDIO_CHECK_RATE.to_string())
            .args(["-f", "s16le", "-"]),
    );
    let (samples, _) = output.stdout.as_chunks::<2>();
    samples.iter().map(|&b| i16::from_le_bytes(b)).collect()
}

/// `inline_limit` as in `process::command`: 0 forces the graph into a file.
fn render_preview(tl: &Timeline, time: f64, inline_limit: usize) -> image::RgbImage {
    let args = preview_args(&tl.project, &tl.infos, time);
    let (mut cmd, _file) = process::command(&ffmpeg(), &args, inline_limit).unwrap();
    let output = run_ok(&mut cmd);
    image::load_from_memory(&output.stdout).unwrap().into_rgb8()
}

fn classify([r, g, b]: [u8; 3]) -> Color {
    match (r, g, b) {
        (200.., ..60, ..60) => Color::Red,
        (..60, ..60, 200..) => Color::Blue,
        (..30, ..30, ..30) => Color::Black,
        _ => Color::Other([r, g, b]),
    }
}

#[test]
fn preview_draws_text_with_special_characters_at_transform() {
    let mut tl = Timeline::new(320, 180, 30.0);
    let clip = tl.project.add_text(Track::Video(0), 0.0);
    let clip = tl.edit(clip);
    clip.transform = Transform {
        x: 25.0,
        y: 75.0,
        ..Transform::default()
    };
    let Content::Text { text } = &mut clip.content else {
        unreachable!()
    };
    text.text = "a:b,'c'[d]%e\\f;\r\nsecond".into();
    text.size = 180.0;
    text.outline = false;
    for inline_limit in [process::MAX_INLINE_COMMAND_LINE, 0] {
        let image = render_preview(&tl, 1.0, inline_limit);
        let lit: Vec<_> = image
            .enumerate_pixels()
            .filter(|(_, _, p)| p.0.iter().all(|&c| c > 128))
            .collect();
        assert!(lit.len() > 100, "{} lit pixels", lit.len());
        let mean = |f: fn(&(u32, u32, &image::Rgb<u8>)) -> u32| {
            lit.iter().map(f).sum::<u32>() as f64 / lit.len() as f64
        };
        let (cx, cy) = (mean(|p| p.0), mean(|p| p.1));
        assert!(
            (cx - 80.0).abs() < 25.0 && (cy - 135.0).abs() < 25.0,
            "{cx} {cy}"
        );
    }
}

#[test]
fn preview_renders_scaled_moved_translucent_clip() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    let clip = tl.clip(&fx.red_16x9, Track::Video(0), 0.0, 3.0);
    tl.edit(clip).transform = Transform {
        x: 100.0,
        y: 100.0,
        scale: 50.0,
        opacity: 50.0,
    };
    let image = render_preview(&tl, 1.0, process::MAX_INLINE_COMMAND_LINE);
    assert_eq!(classify(image.get_pixel(5, 5).0), Color::Black);
    let [r, g, b] = image.get_pixel(300, 170).0;
    assert!(r > 80 && r < 180 && g < 60 && b < 60, "{r} {g} {b}");
}

#[test]
fn preview_text_layer_is_transparent_outside_the_glyphs() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.clip(&fx.red_16x9, Track::Video(0), 0.0, 3.0);
    tl.project.add_text(Track::Video(1), 0.0);
    let image = render_preview(&tl, 1.0, process::MAX_INLINE_COMMAND_LINE);
    assert_eq!(classify(image.get_pixel(5, 5).0), Color::Red);
}

fn still_info() -> MediaInfo {
    MediaInfo {
        duration: 0.0,
        still: true,
        video: media_info(true, false).video,
        has_audio: false,
    }
}

#[test]
fn still_input_loops_without_seeking() {
    let mut tl = Timeline::new(320, 180, 30.0);
    let source = tl.project.add_source("a.png".into());
    tl.infos.insert(source, still_info());
    tl.project.insert_still(source, Track::Video(0), 2.0);
    let graph = tl.build(3.0, 4.0);
    let inputs = graph.inputs.join(" ");
    assert_eq!(inputs, "-loop 1 -framerate 30 -t 1.000000 -i a.png");
    assert!(!graph.filter.contains("[0:a"), "{}", graph.filter);
}

#[test]
fn preview_renders_scaled_moved_still() {
    let mut tl = Timeline::new(320, 180, 30.0);
    let source = tl.project.add_source(fixtures().picture.clone());
    tl.infos.insert(source, still_info());
    let clip = tl.project.insert_still(source, Track::Video(0), 0.0);
    tl.edit(clip).transform = Transform {
        x: 100.0,
        y: 100.0,
        scale: 50.0,
        opacity: 100.0,
    };
    let image = render_preview(&tl, 1.0, process::MAX_INLINE_COMMAND_LINE);
    assert_eq!(classify(image.get_pixel(5, 5).0), Color::Black);
    let lit: u32 = (270..315)
        .flat_map(|x| (140..175).map(move |y| (x, y)))
        .map(|(x, y)| {
            image
                .get_pixel(x, y)
                .0
                .iter()
                .map(|&c| u32::from(c))
                .sum::<u32>()
        })
        .sum();
    assert!(lit / (45 * 35) > 60, "{lit}");
}
