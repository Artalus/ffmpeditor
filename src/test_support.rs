//! Helpers for tests running the real ffmpeg from PATH.

use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::ffmpeg::Ffmpeg;

pub fn ffmpeg() -> Ffmpeg {
    Ffmpeg::new(Path::new("ffmpeg"))
}

/// Polls `cond` until it holds; fails the test after `timeout_secs`.
pub fn wait_until(timeout_secs: u64, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
