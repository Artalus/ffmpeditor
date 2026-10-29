use std::io::{self, Read};
use std::path::Path;
use std::process::Command;

use super::read_full;
use crate::ffmpeg::Ffmpeg;

pub const PEAKS_PER_SECOND: f64 = 100.0;
const SAMPLE_RATE: usize = 8000;
const SAMPLES_PER_PEAK: usize = SAMPLE_RATE / PEAKS_PER_SECOND as usize;
const BYTES_PER_PEAK: usize = SAMPLES_PER_PEAK * 2;
/// Peaks are handed over in batches so the waveform grows on screen while a long file decodes.
const PEAKS_PER_BATCH: usize = 1000;

/// Mono s16le PCM of the source's first audio track on stdout.
pub fn command(ffmpeg: &Ffmpeg, path: &Path) -> Command {
    let mut cmd = ffmpeg.ffmpeg();
    cmd.args(["-v", "error", "-nostats", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1"])
        .args(["-ar", &SAMPLE_RATE.to_string()])
        .args(["-f", "s16le", "-"]);
    cmd
}

pub fn read_peaks(mut pcm: impl Read, mut on_batch: impl FnMut(Vec<f32>)) -> io::Result<()> {
    let mut block = vec![0; BYTES_PER_PEAK * PEAKS_PER_BATCH];
    loop {
        let len = read_full(&mut pcm, &mut block)?;
        if len > 0 {
            on_batch(peaks(&block[..len]));
        }
        if len < block.len() {
            return Ok(());
        }
    }
}

fn peaks(pcm: &[u8]) -> Vec<f32> {
    pcm.chunks(BYTES_PER_PEAK)
        .map(|bucket| {
            let max = bucket
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&s| i16::from_le_bytes(s).unsigned_abs())
                .max()
                .unwrap_or(0);
            f32::from(max) / 32768.0
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn peaks_are_absolute_bucket_maxima() {
        let mut samples = vec![0i16; SAMPLES_PER_PEAK * 2 + 3];
        samples[5] = -16384;
        samples[6] = 8192;
        samples[SAMPLES_PER_PEAK + 1] = i16::MIN;
        samples[SAMPLES_PER_PEAK * 2 + 2] = 16384;
        assert_eq!(peaks(&pcm(&samples)), vec![0.5, 1.0, 0.5]);
    }

    #[test]
    fn peaks_arrive_in_batches() {
        let samples = vec![100i16; SAMPLES_PER_PEAK * (PEAKS_PER_BATCH + 10)];
        let mut batches = Vec::new();
        read_peaks(pcm(&samples).as_slice(), |batch| batches.push(batch.len())).unwrap();
        assert_eq!(batches, vec![PEAKS_PER_BATCH, 10]);

        let mut batches = 0;
        read_peaks(io::empty(), |_| batches += 1).unwrap();
        assert_eq!(batches, 0);
    }
}
