use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use super::{MediaInfo, MediaInfos, Waveform, probe, thumbnails, waveform};
use crate::ffmpeg::{Ffmpeg, Slot};
use crate::model::SourceId;

/// Importing many files at once must not start an ffmpeg per file.
const WORKERS: usize = 2;

/// Owns everything derived from source files; extraction runs on background threads.
pub struct MediaCache {
    ctx: egui::Context,
    ffmpeg: Ffmpeg,
    infos: MediaInfos,
    sources: HashMap<SourceId, SourceState>,
    /// Each `add` takes a fresh ticket; results carrying an older one for the same source are stale.
    next_ticket: u64,
    jobs: Sender<Job>,
    results: Receiver<Update>,
    workers: Arc<Workers>,
}

struct SourceState {
    path: PathBuf,
    ticket: u64,
    error: Option<String>,
    thumbnails: Thumbnails,
    waveform: Option<Waveform>,
    running_jobs: usize,
}

#[derive(Default)]
struct Thumbnails {
    layout: Option<thumbnails::Layout>,
    /// A picture has the one thumbnail, for any source time.
    still: bool,
    textures: Vec<egui::TextureHandle>,
    finished: bool,
}

struct Workers {
    ctx: egui::Context,
    /// Jobs with lower tickets were cleared: workers skip them and their processes get killed.
    first_live_ticket: AtomicU64,
    running: [Slot; WORKERS],
}

struct Job {
    ticket: u64,
    id: SourceId,
    path: PathBuf,
    ffmpeg: Ffmpeg,
    kind: JobKind,
}

#[derive(Clone, Copy)]
enum JobKind {
    Thumbnails(thumbnails::Layout),
    Waveform,
}

struct Update {
    ticket: u64,
    id: SourceId,
    payload: Payload,
}

enum Payload {
    Thumbnail(egui::ColorImage),
    Peaks(Vec<f32>),
    Finished(JobKind, anyhow::Result<()>),
}

impl MediaCache {
    pub fn new(ctx: egui::Context, ffmpeg: Ffmpeg) -> Self {
        let (jobs, job_queue) = mpsc::channel();
        let (results_tx, results) = mpsc::channel();
        let job_queue = Arc::new(Mutex::new(job_queue));
        let workers = Arc::new(Workers::new(ctx.clone()));
        for slot in 0..WORKERS {
            let (workers, job_queue, results_tx) =
                (workers.clone(), job_queue.clone(), results_tx.clone());
            thread::Builder::new()
                .name(format!("media worker {slot}"))
                .spawn(move || workers.run(slot, &job_queue, &results_tx))
                .expect("spawning media worker thread");
        }
        Self {
            ctx,
            ffmpeg,
            infos: HashMap::new(),
            sources: HashMap::new(),
            next_ticket: 0,
            jobs,
            results,
            workers,
        }
    }

    pub fn set_ffmpeg(&mut self, ffmpeg: Ffmpeg) {
        self.ffmpeg = ffmpeg;
    }

    pub fn add(&mut self, id: SourceId, path: &Path) -> anyhow::Result<MediaInfo> {
        if let Some(info) = self.loaded(id, path) {
            return Ok(info.clone());
        }
        let mut source = SourceState::new(path, self.next_ticket);
        self.next_ticket += 1;
        let probed = probe(&self.ffmpeg, path);
        match &probed {
            Ok(info) => {
                self.infos.insert(id, info.clone());
                self.start_extraction(id, info, &mut source);
            }
            Err(e) => {
                self.infos.remove(&id);
                source.error = Some(format!("{e:#}"));
            }
        }
        self.sources.insert(id, source);
        probed
    }

    pub fn clear(&mut self) {
        self.workers.cancel_before(self.next_ticket);
        self.infos.clear();
        self.sources.clear();
    }

    /// Call once per frame: collects finished background work and uploads thumbnails as textures.
    pub fn poll(&mut self) {
        while let Ok(update) = self.results.try_recv() {
            if let Some(source) = self.sources.get_mut(&update.id)
                && source.ticket == update.ticket
            {
                source.apply(&self.ctx, update.id, update.payload);
            }
        }
    }

    pub fn info(&self, id: SourceId) -> Option<&MediaInfo> {
        self.infos.get(&id)
    }

    pub fn infos(&self) -> &MediaInfos {
        &self.infos
    }

    /// Why probing or extraction failed, e.g. missing file.
    pub fn error(&self, id: SourceId) -> Option<&str> {
        self.sources.get(&id)?.error.as_deref()
    }

    /// Nearest extracted thumbnail to `source_time`; None while still extracting.
    pub fn thumbnail(&self, id: SourceId, source_time: f64) -> Option<&egui::TextureHandle> {
        self.sources.get(&id)?.thumbnails.get(source_time)
    }

    /// Grows while extraction runs.
    pub fn waveform(&self, id: SourceId) -> Option<&Waveform> {
        self.sources.get(&id)?.waveform.as_ref()
    }

    /// Importing a file that is already loaded keeps its results; a failed one is retried.
    fn loaded(&self, id: SourceId, path: &Path) -> Option<&MediaInfo> {
        let source = self.sources.get(&id)?;
        if source.path != path || source.error.is_some() {
            return None;
        }
        self.infos.get(&id)
    }

    fn start_extraction(&self, id: SourceId, info: &MediaInfo, source: &mut SourceState) {
        if let Some(video) = &info.video {
            let layout = thumbnails::Layout::new(info.duration, video);
            source.thumbnails.layout = Some(layout);
            source.thumbnails.still = info.still;
            self.queue(id, JobKind::Thumbnails(layout), source);
        }
        if info.has_audio {
            self.queue(id, JobKind::Waveform, source);
        }
    }

    fn queue(&self, id: SourceId, kind: JobKind, source: &mut SourceState) {
        let job = Job {
            ticket: source.ticket,
            id,
            path: source.path.clone(),
            ffmpeg: self.ffmpeg.clone(),
            kind,
        };
        match self.jobs.send(job) {
            Ok(()) => source.running_jobs += 1,
            Err(_) => source.error = Some("media worker threads are gone".into()),
        }
    }
}

impl Drop for MediaCache {
    fn drop(&mut self) {
        // Workers exit once `jobs` is dropped and their current ffmpeg dies.
        self.workers.cancel_before(u64::MAX);
    }
}

impl SourceState {
    fn new(path: &Path, ticket: u64) -> Self {
        Self {
            path: path.to_path_buf(),
            ticket,
            error: None,
            thumbnails: Thumbnails::default(),
            waveform: None,
            running_jobs: 0,
        }
    }

    fn apply(&mut self, ctx: &egui::Context, id: SourceId, payload: Payload) {
        match payload {
            Payload::Thumbnail(image) => {
                let name = format!("thumbnail {} {}", id.0, self.thumbnails.textures.len());
                let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
                self.thumbnails.textures.push(texture);
            }
            Payload::Peaks(peaks) => {
                let waveform = self.waveform.get_or_insert_with(|| Waveform {
                    peaks_per_second: waveform::PEAKS_PER_SECOND,
                    peaks: Vec::new(),
                });
                waveform.peaks.extend(peaks);
            }
            Payload::Finished(kind, result) => {
                self.running_jobs -= 1;
                if let JobKind::Thumbnails(_) = kind {
                    self.thumbnails.finished = true;
                }
                if let Err(e) = result {
                    self.error.get_or_insert(format!("{e:#}"));
                }
            }
        }
    }
}

impl Thumbnails {
    fn get(&self, source_time: f64) -> Option<&egui::TextureHandle> {
        let index = if self.still {
            0
        } else {
            self.layout.as_ref()?.index(source_time)
        };
        // ffmpeg can emit a frame fewer than duration / interval; past the end show the last one.
        let index = if self.finished {
            index.min(self.textures.len().saturating_sub(1))
        } else {
            index
        };
        self.textures.get(index)
    }
}

impl Workers {
    fn new(ctx: egui::Context) -> Self {
        Self {
            ctx,
            first_live_ticket: AtomicU64::new(0),
            running: Default::default(),
        }
    }

    fn run(&self, slot: usize, jobs: &Mutex<Receiver<Job>>, results: &Sender<Update>) {
        while let Some(job) = next_job(jobs) {
            if !self.is_live(job.ticket) {
                continue;
            }
            let send = |payload| {
                let update = Update {
                    ticket: job.ticket,
                    id: job.id,
                    payload,
                };
                // Fails only once the cache is dropped, which also kills the running ffmpeg.
                if results.send(update).is_ok() {
                    self.ctx.request_repaint();
                }
            };
            let live = || self.is_live(job.ticket);
            let result = match job.kind {
                JobKind::Thumbnails(layout) => {
                    let cmd = thumbnails::command(&job.ffmpeg, &job.path, &layout);
                    self.running[slot].run(live, cmd, |rgba| {
                        thumbnails::read_frames(rgba, &layout, |frame| {
                            send(Payload::Thumbnail(frame))
                        })
                    })
                }
                JobKind::Waveform => {
                    let cmd = waveform::command(&job.ffmpeg, &job.path);
                    self.running[slot].run(live, cmd, |pcm| {
                        waveform::read_peaks(pcm, |peaks| send(Payload::Peaks(peaks)))
                    })
                }
            };
            send(Payload::Finished(job.kind, result));
        }
    }

    fn is_live(&self, ticket: u64) -> bool {
        ticket >= self.first_live_ticket.load(Ordering::SeqCst)
    }

    fn cancel_before(&self, ticket: u64) {
        self.first_live_ticket.fetch_max(ticket, Ordering::SeqCst);
        self.running.iter().for_each(Slot::kill);
    }
}

/// Separate function so the queue lock is released before the job runs.
fn next_job(jobs: &Mutex<Receiver<Job>>) -> Option<Job> {
    jobs.lock().unwrap().recv().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self, fixtures, wait_until};

    fn cache() -> MediaCache {
        MediaCache::new(egui::Context::default(), test_support::ffmpeg())
    }

    fn is_loading(cache: &MediaCache, id: SourceId) -> bool {
        cache.sources.get(&id).is_some_and(|s| s.running_jobs > 0)
    }

    fn wait_for_extraction(cache: &mut MediaCache, ids: &[SourceId]) {
        wait_until(30, || {
            cache.poll();
            ids.iter().all(|&id| !is_loading(cache, id))
        });
    }

    #[test]
    fn extracts_thumbnails_and_waveforms_in_background() {
        let fx = fixtures();
        let mut cache = cache();
        let (both, video, audio) = (SourceId(1), SourceId(2), SourceId(3));
        cache.add(both, &fx.testsrc).unwrap();
        cache.add(video, &fx.video_only).unwrap();
        cache.add(audio, &fx.audio_only).unwrap();
        wait_for_extraction(&mut cache, &[both, video, audio]);

        for id in [both, video, audio] {
            assert_eq!(cache.error(id), None);
        }
        assert_eq!(cache.infos().len(), 3);
        let first = cache.thumbnail(both, 0.0).unwrap();
        assert_eq!(first.size(), [96, 72]);
        assert_ne!(first.id(), cache.thumbnail(both, 2.0).unwrap().id());
        assert!(cache.thumbnail(both, 1000.0).is_some());
        assert!(cache.thumbnail(video, 1.0).is_some());
        assert!(cache.thumbnail(audio, 1.0).is_none());
        assert!(cache.waveform(both).unwrap().peak(0.0, 3.0) > 0.05);
        assert!(cache.waveform(audio).unwrap().peak(0.0, 2.0) > 0.05);
        assert!(cache.waveform(video).is_none());

        let peaks = cache.waveform(both).unwrap().peaks.len();
        cache.add(both, &fx.testsrc).unwrap();
        assert!(!is_loading(&cache, both));
        assert_eq!(cache.waveform(both).unwrap().peaks.len(), peaks);
    }

    #[test]
    fn picture_has_one_thumbnail_and_no_waveform() {
        let mut cache = cache();
        let id = SourceId(1);
        cache.add(id, &fixtures().picture).unwrap();
        wait_for_extraction(&mut cache, &[id]);
        assert_eq!(cache.error(id), None);
        let first = cache.thumbnail(id, 0.0).unwrap().id();
        assert_eq!(first, cache.thumbnail(id, 42.0).unwrap().id());
        assert!(cache.waveform(id).is_none());
    }

    #[test]
    fn probe_failure_is_reported() {
        let mut cache = cache();
        let id = SourceId(1);
        assert!(cache.add(id, Path::new("missing.mp4")).is_err());
        assert!(cache.error(id).unwrap().contains("not found"));
        assert!(cache.info(id).is_none());
        assert!(!is_loading(&cache, id));
    }

    #[test]
    fn clear_discards_results_of_running_jobs() {
        let fx = fixtures();
        let mut cache = cache();
        let id = SourceId(1);
        cache.add(id, &fx.testsrc).unwrap();
        cache.clear();
        assert!(cache.infos().is_empty());

        cache.add(id, &fx.audio_only).unwrap();
        wait_for_extraction(&mut cache, &[id]);
        assert_eq!(cache.error(id), None);
        assert!(cache.thumbnail(id, 0.0).is_none());
        let peaks = cache.waveform(id).unwrap().peaks.len();
        assert!(peaks <= 210, "stale peaks leaked in: {peaks}");
    }
}
