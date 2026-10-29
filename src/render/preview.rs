use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::Context;

use super::graph::{self, FrameRate, Target};
use super::{MediaInfos, process};
use crate::ffmpeg::{Ffmpeg, Slot};
use crate::model::{Project, Span};

const MAX_PREVIEW_SIZE: (u32, u32) = (960, 540);
const TEXTURE_OPTIONS: egui::TextureOptions = egui::TextureOptions::LINEAR;

/// Renders the composited frame at the playhead in the background; newest request wins.
pub struct Previewer {
    ctx: egui::Context,
    shared: Arc<Shared>,
    frames: Receiver<Result<egui::ColorImage, String>>,
    last_request: Option<Request>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct Request {
    ffmpeg: Ffmpeg,
    args: Vec<String>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
    /// Killed on drop, so no ffmpeg outlives the previewer.
    running: Slot,
}

#[derive(Default)]
struct State {
    pending: Option<Request>,
    rendering: bool,
    closed: bool,
}

/// ffmpeg arguments writing the single frame at timeline `time` as PNG to stdout.
/// The frame is downscaled to fit 960x540, since rendering it is what limits scrubbing speed.
pub fn preview_args(project: &Project, infos: &MediaInfos, time: f64) -> Vec<String> {
    let settings = &project.settings;
    let rate = FrameRate::from_fps(settings.fps);
    let frame = rate.frame_containing(time.max(0.0));
    let (width, height) = preview_size(settings.width, settings.height);
    let graph = graph::build(
        project,
        infos,
        &Target {
            range: Span {
                start: rate.time(frame),
                end: rate.time(frame + 1),
            },
            width,
            height,
            audio: false,
            input_lookahead: 1.0,
        },
    );
    let mut args = vec!["-loglevel".into(), "error".into()];
    args.extend(graph.inputs);
    args.extend(
        [
            "-filter_complex",
            &graph.filter,
            "-map",
            "[v]",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-c:v",
            "png",
            "-",
        ]
        .map(String::from),
    );
    args
}

impl Previewer {
    pub fn new(ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared::default());
        let (frames_tx, frames) = mpsc::channel();
        thread::spawn({
            let shared = shared.clone();
            let ctx = ctx.clone();
            move || render_loop(&shared, &frames_tx, &ctx)
        });
        Self {
            ctx,
            shared,
            frames,
            last_request: None,
            texture: None,
            error: None,
        }
    }

    /// Cheap to call every frame: a request rendering the same as the previous one is ignored.
    pub fn request(&mut self, ffmpeg: &Ffmpeg, project: &Project, infos: &MediaInfos, time: f64) {
        let request = Request {
            ffmpeg: ffmpeg.clone(),
            args: preview_args(project, infos, time),
        };
        if self.last_request.as_ref() == Some(&request) {
            return;
        }
        self.last_request = Some(request.clone());
        self.shared.state.lock().unwrap().pending = Some(request);
        self.shared.wake.notify_one();
    }

    /// Call once per frame: picks up a finished frame and uploads it as a texture.
    pub fn poll(&mut self) {
        let Some(frame) = self.frames.try_iter().last() else {
            return;
        };
        match frame {
            Ok(image) => {
                self.error = None;
                match &mut self.texture {
                    Some(texture) => texture.set(image, TEXTURE_OPTIONS),
                    None => {
                        self.texture =
                            Some(self.ctx.load_texture("preview", image, TEXTURE_OPTIONS))
                    }
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.texture.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_busy(&self) -> bool {
        let state = self.shared.state.lock().unwrap();
        state.pending.is_some() || state.rendering
    }
}

impl Drop for Previewer {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().closed = true;
        self.shared.wake.notify_one();
        self.shared.running.kill();
    }
}

fn render_loop(
    shared: &Shared,
    frames: &Sender<Result<egui::ColorImage, String>>,
    ctx: &egui::Context,
) {
    while let Some(request) = shared.next_request() {
        let frame = render(shared, &request).map_err(|e| format!("{e:#}"));
        let delivered = frames.send(frame).is_ok();
        shared.state.lock().unwrap().rendering = false;
        if !delivered {
            break;
        }
        ctx.request_repaint();
    }
}

fn render(shared: &Shared, request: &Request) -> anyhow::Result<egui::ColorImage> {
    let (cmd, _graph_file) = process::command(&request.ffmpeg, &request.args)?;
    let png = shared.running.run(
        || !shared.state.lock().unwrap().closed,
        cmd,
        |mut stdout| {
            let mut png = Vec::new();
            stdout.read_to_end(&mut png).map(|_| png)
        },
    )?;
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .context("decoding preview frame")?
        .into_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

fn preview_size(width: u32, height: u32) -> (u32, u32) {
    let (max_width, max_height) = MAX_PREVIEW_SIZE;
    let scale = (f64::from(max_width) / f64::from(width))
        .min(f64::from(max_height) / f64::from(height))
        .min(1.0);
    let even = |v: u32| ((f64::from(v) * scale / 2.0).round() as u32 * 2).max(2);
    (even(width), even(height))
}

impl Shared {
    /// Blocks until there is something to render; None once the previewer is dropped.
    fn next_request(&self) -> Option<Request> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.closed {
                return None;
            }
            if let Some(request) = state.pending.take() {
                state.rendering = true;
                return Some(request);
            }
            state = self.wake.wait(state).unwrap();
        }
    }
}
