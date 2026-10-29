use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

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

fn sibling_ffprobe(ffmpeg: &Path) -> PathBuf {
    let name = match ffmpeg.extension() {
        Some(ext) => format!("ffprobe.{}", ext.to_string_lossy()),
        None => "ffprobe".to_owned(),
    };
    match ffmpeg.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

fn command(program: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null());
    hide_console(&mut cmd);
    cmd
}

#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    // Without this every spawned ffmpeg flashes a console window in the release (GUI subsystem) build.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

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
