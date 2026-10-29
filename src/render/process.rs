//! Spawning ffmpeg with possibly huge filter graphs and reading its output.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context;

use crate::ffmpeg::Ffmpeg;

/// Windows caps a command line at 32767 chars; graphs with many clips get there.
pub const MAX_INLINE_COMMAND_LINE: usize = 30_000;

/// Filter graph moved out of the command line; deleted when dropped, so keep it until ffmpeg exits.
pub struct GraphFile {
    path: PathBuf,
}

pub fn command(
    ffmpeg: &Ffmpeg,
    args: &[String],
    limit: usize,
) -> anyhow::Result<(Command, Option<GraphFile>)> {
    let mut args = args.to_vec();
    let graph_file = if command_line_len(ffmpeg, &args) > limit {
        move_graph_to_file(&mut args)?
    } else {
        None
    };
    let mut cmd = ffmpeg.ffmpeg();
    cmd.args(&args);
    Ok((cmd, graph_file))
}

/// Upper estimate: quotes and a separator around every argument.
fn command_line_len(ffmpeg: &Ffmpeg, args: &[String]) -> usize {
    ffmpeg.ffmpeg.as_os_str().len() + args.iter().map(|a| a.len() + 3).sum::<usize>()
}

fn move_graph_to_file(args: &mut [String]) -> anyhow::Result<Option<GraphFile>> {
    let Some(i) = args.iter().position(|a| a == "-filter_complex") else {
        return Ok(None);
    };
    let file = GraphFile::write(&args[i + 1])?;
    args[i] = "-/filter_complex".into();
    args[i + 1] = file.path.to_string_lossy().into_owned();
    Ok(Some(file))
}

impl GraphFile {
    fn write(graph: &str) -> anyhow::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "ffmpeditor-{}-{}.filtergraph",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, graph).with_context(|| format!("writing {}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for GraphFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn long_command_line_moves_graph_to_file() {
        let ffmpeg = Ffmpeg::new(Path::new("ffmpeg"));
        let args = ["-filter_complex", "graph", "-y", "out.mp4"].map(String::from);
        let (cmd, file) = command(&ffmpeg, &args, 0).unwrap();
        let cmd_args: Vec<_> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
        let i = cmd_args
            .iter()
            .position(|a| *a == "-/filter_complex")
            .unwrap();
        assert_eq!(std::fs::read_to_string(cmd_args[i + 1]).unwrap(), "graph");
        drop(file);
        assert!(!Path::new(cmd_args[i + 1]).exists());

        let (cmd, file) = command(&ffmpeg, &args, MAX_INLINE_COMMAND_LINE).unwrap();
        assert!(file.is_none());
        assert!(cmd.get_args().any(|a| a == "-filter_complex"));
    }
}
