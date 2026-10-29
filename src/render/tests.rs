//! Argument checks plus end-to-end runs against the real ffmpeg on PATH.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::*;
use crate::ffmpeg::Ffmpeg;
use crate::media::VideoInfo;
use crate::model::{ClipId, Project, Span, Track};

struct Timeline {
    project: Project,
    infos: MediaInfos,
}

struct Fixtures {
    testsrc: PathBuf,
    red_16x9: PathBuf,
}

#[test]
fn export_args_encode_with_project_settings() {
    let mut tl = Timeline::new(1280, 720, 60.0);
    tl.project.settings.video_codec = "libx265".into();
    tl.project.settings.video_bitrate_kbps = 8000;
    tl.project.settings.audio_bitrate_kbps = 128;
    let args = export_args(
        &tl.project,
        &tl.infos,
        Span::new(0.0, 1.0),
        Path::new("out.mp4"),
    );
    let tail = args.iter().skip_while(|a| *a != "-map").map(String::as_str);
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
}

#[test]
fn faststart_only_for_quicktime_containers() {
    let tl = Timeline::new(1280, 720, 60.0);
    let args = |output: &str| {
        export_args(
            &tl.project,
            &tl.infos,
            Span::new(0.0, 1.0),
            Path::new(output),
        )
    };
    assert!(args("a.MOV").contains(&"+faststart".to_owned()));
    assert!(!args("a.mkv").contains(&"+faststart".to_owned()));
}

#[test]
fn export_renders_overlapping_clips_in_subrange() {
    let fx = fixtures();
    let mut tl = Timeline::new(320, 180, 30.0);
    tl.source_clip(&fx.testsrc, Track::Video(0), 0.0, 3.0);
    let second = tl.source_clip(&fx.testsrc, Track::Video(0), 2.0, 3.0);
    tl.edit(second).audio.mute_regions = vec![Span::new(0.5, 1.0)];
    let output = output_path("overlapping.mp4");
    let args = export_args(&tl.project, &tl.infos, Span::new(1.0, 4.5), &output);
    let status = wait(&ExportJob::start(&ffmpeg(), args, 3.5).unwrap());
    assert_eq!(status, ExportStatus::Finished);
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
    let args = export_args(&tl.project, &tl.infos, Span::new(0.0, 0.5), &output);
    assert_eq!(
        wait(&ExportJob::start(&ffmpeg(), args, 0.5).unwrap()),
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
    let clip = tl.source_clip(&fx.red_16x9, Track::Video(0), 1.0, 2.0);
    tl.edit(clip).audio.mute_regions = vec![Span::new(0.5, 1.0)];
    let output = output_path("audio.mkv");
    let args = export_args(&tl.project, &tl.infos, Span::new(0.0, 3.0), &output);
    assert_eq!(
        wait(&ExportJob::start(&ffmpeg(), args, 3.0).unwrap()),
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
    let args = export_args(&tl.project, &tl.infos, Span::new(0.0, 1.0), &output);
    let status = wait(&ExportJob::start(&ffmpeg(), args, 1.0).unwrap());
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
    let mut job = ExportJob::start(&ffmpeg(), args, 600.0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while job.status() == (ExportStatus::Running { progress: 0.0 }) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        matches!(job.status(), ExportStatus::Running { progress } if progress > 0.0 && progress < 1.0)
    );
    job.cancel();
    assert_eq!(job.status(), ExportStatus::Cancelled);
    drop(job);
    // A launcher shim (scoop) in between kills the real ffmpeg only after it exits itself,
    // so the output may stay locked for a moment.
    let deadline = Instant::now() + Duration::from_secs(5);
    while let Err(e) = std::fs::remove_file(&output) {
        assert!(Instant::now() < deadline, "ffmpeg still holds output: {e}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

impl Timeline {
    fn new(width: u32, height: u32, fps: f64) -> Self {
        let mut project = Project::default();
        project.settings.width = width;
        project.settings.height = height;
        project.settings.fps = fps;
        Self {
            project,
            infos: MediaInfos::new(),
        }
    }

    fn clip(&mut self, path: &str, track: Track, start: f64, duration: f64) -> ClipId {
        self.source_clip(Path::new(path), track, start, duration)
    }

    /// Media info matches the fixtures: 3 s, 30 fps, 320x240, with audio.
    fn source_clip(&mut self, path: &Path, track: Track, start: f64, duration: f64) -> ClipId {
        let source = self.project.add_source(path.to_path_buf());
        let info = MediaInfo {
            duration: 3.0,
            video: Some(VideoInfo {
                width: 320,
                height: 240,
                fps: 30.0,
            }),
            has_audio: true,
        };
        self.infos.insert(source, info);
        let id = self.project.append_clip(source, track, duration);
        self.project.move_clip(id, start, track);
        id
    }

    fn edit(&mut self, id: ClipId) -> &mut crate::model::Clip {
        self.project.clip_mut(id).unwrap()
    }
}

fn ffmpeg() -> Ffmpeg {
    Ffmpeg::new(Path::new("ffmpeg"))
}

/// Generated once per test run; stored under target/ so concurrent runs elsewhere don't collide.
fn fixtures() -> &'static Fixtures {
    static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
    FIXTURES.get_or_init(|| Fixtures {
        testsrc: fixture(
            "testsrc.mp4",
            "testsrc2=size=320x240:rate=30:duration=3",
            "sine=frequency=440:duration=3",
        ),
        red_16x9: fixture(
            "red-16x9.mp4",
            "color=c=red:size=320x180:rate=30:duration=3",
            "sine=frequency=440:duration=3",
        ),
    })
}

fn fixture(name: &str, video: &str, audio: &str) -> PathBuf {
    let path = fixtures_dir().join(name);
    if path.exists() {
        return path;
    }
    let partial = fixtures_dir().join(format!("{}.{name}", std::process::id()));
    let output = ffmpeg()
        .ffmpeg()
        .args([
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            video,
            "-f",
            "lavfi",
            "-i",
            audio,
        ])
        .args(["-c:v", "libx264", "-c:a", "aac", "-shortest", "-f", "mp4"])
        .arg(&partial)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Rename is atomic, so a concurrent run never sees a half-written fixture.
    if std::fs::rename(&partial, &path).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    path
}

fn fixtures_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/render-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn output_path(name: &str) -> PathBuf {
    fixtures_dir().join(format!("out-{}-{name}", std::process::id()))
}

fn wait(job: &ExportJob) -> ExportStatus {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match job.status() {
            ExportStatus::Running { .. } if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20))
            }
            status => return status,
        }
    }
}

#[derive(Debug)]
struct Stream {
    kind: String,
    duration: Option<f64>,
    frames: Option<f64>,
}

fn probe_streams(path: &Path) -> Vec<Stream> {
    let output = ffmpeg()
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
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
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
    let output = ffmpeg()
        .ffmpeg()
        .args(["-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar"])
        .arg(AUDIO_CHECK_RATE.to_string())
        .args(["-f", "s16le", "-"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (samples, _) = output.stdout.as_chunks::<2>();
    samples.iter().map(|&b| i16::from_le_bytes(b)).collect()
}
