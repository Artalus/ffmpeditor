//! Helpers for tests running the real ffmpeg from PATH.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use crate::ffmpeg::Ffmpeg;
use crate::media::{MediaInfo, VideoInfo};
use crate::model::{ClipId, Project, Track};

const H264_AAC: &[&str] = &[
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-g",
    "30",
    "-c:a",
    "aac",
    "-shortest",
];

pub struct Fixtures {
    /// 3 s of 320x240 @ 30 fps with a keyframe every second, plus a 440 Hz tone.
    pub testsrc: PathBuf,
    /// 3 s of red 320x180 @ 30 fps plus a 440 Hz tone.
    pub red_16x9: PathBuf,
    /// 3 s of blue 320x240 @ 30 fps plus an 880 Hz tone.
    pub blue_4x3: PathBuf,
    /// 2 s of 320x240 @ 30 fps.
    pub video_only: PathBuf,
    /// 2 s of a 440 Hz tone.
    pub audio_only: PathBuf,
}

pub fn ffmpeg() -> Ffmpeg {
    Ffmpeg::new(Path::new("ffmpeg"))
}

/// Runs `cmd` to the end, failing the test with its stderr unless it succeeds.
pub fn run_ok(cmd: &mut Command) -> Output {
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Info of a source like the fixtures: 3 s of 320x240 @ 30 fps video and/or audio.
pub fn media_info(video: bool, audio: bool) -> MediaInfo {
    MediaInfo {
        duration: 3.0,
        video: video.then_some(VideoInfo {
            width: 320,
            height: 240,
            fps: 30.0,
        }),
        has_audio: audio,
    }
}

/// One source and a clip per `(track, start, duration)`; clips get ids 1, 2, ... in order.
pub fn project<const N: usize>(clips: [(Track, f64, f64); N]) -> (Project, [ClipId; N]) {
    let mut project = Project::default();
    let source = project.add_source("a.mp4".into());
    let ids = clips.map(|(track, start, duration)| {
        let id = project.append_clip(source, track, duration);
        project.move_clip(id, start, track);
        id
    });
    project.cover_clip_tracks();
    (project, ids)
}

/// Polls `cond` until it holds; fails the test after `timeout_secs`.
pub fn wait_until(timeout_secs: u64, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Generated on first use and kept under target/ between runs: delete the directory to regenerate.
pub fn fixtures() -> &'static Fixtures {
    static FIXTURES: OnceLock<Fixtures> = OnceLock::new();
    FIXTURES.get_or_init(|| Fixtures {
        testsrc: fixture(
            "testsrc.mp4",
            &[
                "testsrc2=size=320x240:rate=30:duration=3",
                "sine=frequency=440:duration=3",
            ],
            H264_AAC,
        ),
        red_16x9: fixture(
            "red-16x9.mp4",
            &[
                "color=c=red:size=320x180:rate=30:duration=3",
                "sine=frequency=440:duration=3",
            ],
            H264_AAC,
        ),
        blue_4x3: fixture(
            "blue-4x3.mp4",
            &[
                "color=c=blue:size=320x240:rate=30:duration=3",
                "sine=frequency=880:duration=3",
            ],
            H264_AAC,
        ),
        video_only: fixture(
            "video-only.mp4",
            &["testsrc2=size=320x240:rate=30:duration=2"],
            H264_AAC,
        ),
        audio_only: fixture("audio-only.wav", &["sine=frequency=440:duration=2"], &[]),
    })
}

/// Unique per test process, so parallel runs never write the same file.
pub fn output_path(name: &str) -> PathBuf {
    fixtures_dir().join(format!("out-{}-{name}", std::process::id()))
}

fn fixture(name: &str, lavfi_sources: &[&str], codec_args: &[&str]) -> PathBuf {
    let path = fixtures_dir().join(name);
    if path.exists() {
        return path;
    }
    let partial = fixtures_dir().join(format!("{}.{name}", std::process::id()));
    let mut cmd = ffmpeg().ffmpeg();
    cmd.args(["-loglevel", "error", "-y"]);
    for source in lavfi_sources {
        cmd.args(["-f", "lavfi", "-i", source]);
    }
    run_ok(cmd.args(codec_args).arg(&partial));
    // Rename is atomic, so a concurrent run never sees a half-written fixture.
    if std::fs::rename(&partial, &path).is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    path
}

fn fixtures_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
