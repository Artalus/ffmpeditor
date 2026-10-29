use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::Context;

use super::ExportStatus;
use super::graph::{self, FrameRate, Target};
use super::process::{self, GraphFile};
use crate::ffmpeg::{self, Ffmpeg};
use crate::media::MediaInfos;
use crate::model::{Project, Span};

pub struct ExportJob {
    process: Mutex<Process>,
    progress: Arc<Mutex<f32>>,
    stderr_tail: Arc<Mutex<String>>,
}

struct Process {
    child: Child,
    readers: [JoinHandle<()>; 2],
    outcome: Option<ExportStatus>,
    _graph_file: Option<GraphFile>,
}

/// ffmpeg arguments (without the program) rendering timeline `range` of `project` into `output`.
/// The range is snapped to the project frame grid.
pub fn export_args(
    project: &Project,
    infos: &MediaInfos,
    range: Span,
    output: &Path,
) -> Vec<String> {
    let settings = &project.settings;
    let graph = graph::build(
        project,
        infos,
        &Target {
            range,
            width: settings.width,
            height: settings.height,
            audio: true,
            input_lookahead: 0.0,
        },
    );
    let mut args = vec!["-loglevel".into(), "warning".into()];
    args.extend(graph.inputs);
    args.extend([
        "-filter_complex".into(),
        graph.filter,
        "-map".into(),
        "[v]".into(),
        "-map".into(),
        "[a]".into(),
        "-c:v".into(),
        settings.video_codec.clone(),
        "-b:v".into(),
        format!("{}k", settings.video_bitrate_kbps),
        "-c:a".into(),
        settings.audio_codec.clone(),
        "-b:a".into(),
        format!("{}k", settings.audio_bitrate_kbps),
        "-r".into(),
        FrameRate::from_fps(settings.fps).to_string(),
        "-pix_fmt".into(),
        "yuv420p".into(),
    ]);
    if is_quicktime(output) {
        args.extend(["-movflags".into(), "+faststart".into()]);
    }
    args.extend([
        "-progress".into(),
        "pipe:1".into(),
        "-nostats".into(),
        "-y".into(),
        output.to_string_lossy().into_owned(),
    ]);
    args
}

impl ExportJob {
    /// `duration` is the length of the rendered range, used to compute progress.
    pub fn start(ffmpeg: &Ffmpeg, args: Vec<String>, duration: f64) -> anyhow::Result<Self> {
        let (mut cmd, graph_file) = process::command(ffmpeg, &args)?;
        let mut child = cmd
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", ffmpeg.ffmpeg.display()))?;
        let progress = Arc::new(Mutex::new(0.0));
        let stderr_tail = Arc::<Mutex<String>>::default();
        let readers = [
            read_progress(child.stdout.take().unwrap(), duration, progress.clone()),
            read_stderr(child.stderr.take().unwrap(), stderr_tail.clone()),
        ];
        Ok(Self {
            process: Mutex::new(Process {
                child,
                readers,
                outcome: None,
                _graph_file: graph_file,
            }),
            progress,
            stderr_tail,
        })
    }

    pub fn status(&self) -> ExportStatus {
        let mut process = self.process.lock().unwrap();
        if let Some(outcome) = &process.outcome {
            return outcome.clone();
        }
        let outcome = match process.child.try_wait() {
            // Readers are still draining the pipes, the stderr tail may be incomplete.
            Ok(Some(_)) if !process.readers.iter().all(JoinHandle::is_finished) => None,
            Ok(Some(exit)) if exit.success() => Some(ExportStatus::Finished),
            Ok(Some(exit)) => Some(ExportStatus::Failed(ffmpeg::failure(
                exit,
                &self.stderr_tail.lock().unwrap(),
            ))),
            Ok(None) => None,
            Err(e) => Some(ExportStatus::Failed(format!("waiting for ffmpeg: {e}"))),
        };
        match outcome {
            Some(outcome) => {
                process.outcome = Some(outcome.clone());
                outcome
            }
            None => ExportStatus::Running {
                progress: *self.progress.lock().unwrap(),
            },
        }
    }

    pub fn cancel(&mut self) {
        let process = self.process.get_mut().unwrap();
        if process.outcome.is_none() && matches!(process.child.try_wait(), Ok(None)) {
            let _ = process.child.kill();
            let _ = process.child.wait();
            process.outcome = Some(ExportStatus::Cancelled);
        }
    }
}

impl Drop for ExportJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn read_progress(stdout: ChildStdout, duration: f64, progress: Arc<Mutex<f32>>) -> JoinHandle<()> {
    thread::spawn(move || {
        for line in ffmpeg::lines(stdout) {
            if let Some(fraction) = progress_fraction(&line, duration) {
                *progress.lock().unwrap() = fraction;
            }
        }
    })
}

fn read_stderr(stderr: ChildStderr, tail: Arc<Mutex<String>>) -> JoinHandle<()> {
    thread::spawn(move || *tail.lock().unwrap() = ffmpeg::stderr_tail(stderr))
}

fn progress_fraction(line: &str, duration: f64) -> Option<f32> {
    if line == "progress=end" {
        return Some(1.0);
    }
    let us: f64 = line.strip_prefix("out_time_us=")?.parse().ok()?;
    Some((us / 1e6 / duration.max(1e-9)).clamp(0.0, 1.0) as f32)
}

/// Containers that need the index moved to the front to start playing before fully downloaded.
fn is_quicktime(output: &Path) -> bool {
    let ext = output
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    matches!(ext.as_deref(), Some("mp4" | "mov" | "m4v"))
}
