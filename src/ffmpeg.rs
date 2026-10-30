use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;

use anyhow::{Context, bail};

const STDERR_TAIL_LINES: usize = 30;

/// Locations of the ffmpeg and ffprobe binaries; ffprobe is expected next to ffmpeg.
#[derive(Clone, Debug, PartialEq)]
pub struct Ffmpeg {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Ffmpeg {
    pub fn new(ffmpeg: &Path) -> Self {
        Self {
            ffmpeg: ffmpeg.to_path_buf(),
            ffprobe: sibling_ffprobe(ffmpeg),
        }
    }

    pub fn ffmpeg(&self) -> Command {
        let mut cmd = command(&self.ffmpeg);
        cmd.args(["-hide_banner", "-nostdin"]);
        cmd
    }

    pub fn ffprobe(&self) -> Command {
        let mut cmd = command(&self.ffprobe);
        cmd.arg("-hide_banner");
        cmd
    }

    /// First line of `ffmpeg -version`; doubles as a check that the binary runs.
    pub fn version(&self) -> anyhow::Result<String> {
        let output = command(&self.ffmpeg)
            .arg("-version")
            .output()
            .with_context(|| format!("running {}", self.ffmpeg.display()))?;
        if !output.status.success() {
            bail!("{} -version failed", self.ffmpeg.display());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.lines().next().unwrap_or_default().to_owned())
    }
}

/// Lines of ffmpeg output. Lossy: paths in messages may not be UTF-8, and stopping at a bad line
/// would leave the pipe undrained and block ffmpeg.
pub fn lines(reader: impl Read) -> impl Iterator<Item = String> {
    BufReader::new(reader)
        .split(b'\n')
        .map_while(Result::ok)
        .map(|line| String::from_utf8_lossy(&line).trim_end().to_owned())
}

/// Drains `stderr`, keeping its last lines.
pub fn stderr_tail(stderr: impl Read) -> String {
    let mut tail = VecDeque::new();
    for line in lines(stderr).filter(|line| !line.is_empty()) {
        if tail.len() == STDERR_TAIL_LINES {
            tail.pop_front();
        }
        tail.push_back(line);
    }
    tail.make_contiguous().join("\n")
}

pub fn failure(status: ExitStatus, stderr_tail: &str) -> String {
    match stderr_tail.trim() {
        "" => format!("ffmpeg failed: {status}"),
        tail => format!("ffmpeg failed: {status}: {tail}"),
    }
}

/// Where a worker thread parks its running ffmpeg, so another thread can kill it.
#[derive(Default)]
pub struct Slot(Mutex<Option<Child>>);

impl Slot {
    /// Runs `cmd`, handing its stdout to `read`. Does nothing unless `live` holds; it is checked
    /// under the slot lock, so a concurrent `kill` either sees the child or ran after `live` went false.
    pub fn run<T>(
        &self,
        live: impl FnOnce() -> bool,
        mut cmd: Command,
        read: impl FnOnce(ChildStdout) -> io::Result<T>,
    ) -> anyhow::Result<T> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let (stdout, stderr) = {
            let mut parked = self.parked();
            if !live() {
                bail!("cancelled");
            }
            let program = Path::new(cmd.get_program()).display().to_string();
            let mut child = cmd.spawn().with_context(|| format!("running {program}"))?;
            let stdout = child.stdout.take().expect("stdout is piped");
            let stderr = child.stderr.take().expect("stderr is piped");
            *parked = Some(child);
            (stdout, stderr)
        };
        // Drained concurrently, or a chatty ffmpeg blocks on a full stderr pipe.
        let stderr = thread::spawn(move || stderr_tail(stderr));
        let read = read(stdout);
        let mut child = self.parked().take().expect("parked above");
        let status = child.wait()?;
        let stderr = stderr.join().unwrap_or_default();
        let output = read.context("reading ffmpeg output")?;
        if !status.success() {
            bail!(failure(status, &stderr));
        }
        Ok(output)
    }

    pub fn kill(&self) {
        if let Some(child) = self.parked().as_mut() {
            // Errors only if it already exited; `run` reaps it either way.
            let _ = child.kill();
        }
    }

    #[cfg(test)]
    pub fn is_running(&self) -> bool {
        self.parked().is_some()
    }

    /// A worker panicking mid-job must not take the UI thread down with it.
    fn parked(&self) -> MutexGuard<'_, Option<Child>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn sibling_ffprobe(ffmpeg: &Path) -> PathBuf {
    let mut ffprobe = ffmpeg.with_file_name("ffprobe");
    if let Some(ext) = ffmpeg.extension() {
        ffprobe.set_extension(ext);
    }
    ffprobe
}

fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Without this every spawned ffmpeg flashes a console window in the release (GUI subsystem) build.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_support::{self, wait_until};

    #[test]
    fn ffprobe_is_found_next_to_ffmpeg() {
        assert_eq!(sibling_ffprobe(Path::new("ffmpeg")), Path::new("ffprobe"));
        assert_eq!(
            sibling_ffprobe(Path::new("C:/tools/ffmpeg.exe")),
            Path::new("C:/tools/ffprobe.exe")
        );
        assert_eq!(
            sibling_ffprobe(Path::new("/usr/bin/ffmpeg")),
            Path::new("/usr/bin/ffprobe")
        );
    }

    #[test]
    fn kill_stops_the_parked_ffmpeg() {
        let endless = || {
            let mut cmd = test_support::ffmpeg().ffmpeg();
            cmd.args([
                "-v", "error", "-f", "lavfi", "-i", "sine", "-f", "s16le", "-",
            ]);
            cmd
        };
        let slot = Arc::new(Slot::default());
        let running = {
            let slot = slot.clone();
            thread::spawn(move || {
                slot.run(
                    || true,
                    endless(),
                    |mut pcm| io::copy(&mut pcm, &mut io::sink()),
                )
            })
        };
        wait_until(30, || slot.is_running());
        slot.kill();
        assert!(running.join().unwrap().is_err());
        assert!(!slot.is_running());

        assert!(slot.run(|| false, endless(), |_| Ok(())).is_err());
        assert!(!slot.is_running());
    }
}
