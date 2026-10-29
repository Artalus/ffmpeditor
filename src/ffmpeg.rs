use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

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
    use super::*;

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
}
