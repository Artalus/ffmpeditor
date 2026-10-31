//! Filter graph shared by export and preview.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::media::MediaInfos;
use crate::model::{Clip, Project, Span, TextProps, Transform};

const SAMPLE_RATE: u32 = 48_000;
/// Text size is in points of this many per frame height.
const POINTS_PER_FRAME: f64 = 1080.0;

/// What to render; `range` gets snapped to the project frame grid.
pub struct Target {
    pub range: Span,
    pub width: u32,
    pub height: u32,
    pub audio: bool,
    /// Lets inputs run past the range end, so a single-frame range still gets a source frame
    /// when the source frame rate is lower than the project's.
    pub input_lookahead: f64,
}

/// Input arguments plus a `-filter_complex` graph producing `[v]` and, with audio, `[a]`.
pub struct Graph {
    pub inputs: Vec<String>,
    pub filter: String,
}

/// Exact rational, so NTSC rates stay on ffmpeg's own frame grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameRate {
    num: u64,
    den: u64,
}

struct Layer<'a> {
    clip: &'a Clip,
    /// Absent for text, which has no ffmpeg input.
    input: Option<Input<'a>>,
    /// Part of the clip inside the range, timeline time.
    visible: Span,
    input_end: f64,
    video: bool,
    audio: bool,
    overlap_before: f64,
    overlap_after: f64,
}

struct Input<'a> {
    index: usize,
    path: &'a Path,
}

impl Graph {
    /// ffmpeg arguments up to and including the video output mapping.
    pub fn into_args(self, loglevel: &str) -> Vec<String> {
        let mut args = vec!["-loglevel".into(), loglevel.into()];
        args.extend(self.inputs);
        args.extend(["-filter_complex", &self.filter, "-map", "[v]"].map(String::from));
        args
    }
}

pub fn build(project: &Project, infos: &MediaInfos, target: &Target) -> Graph {
    let rate = FrameRate::from_fps(project.settings.fps);
    let range = rate.snap(target.range);
    let layers = layers(project, infos, target, range);
    let inputs = layers
        .iter()
        .filter_map(|layer| input_args(layer, rate))
        .flatten()
        .collect();
    let mut chains = vec![base_video(target, rate, range)];
    chains.extend(video_layers(&layers).map(|(i, layer)| clip_video(i, layer, target, rate)));
    chains.push(overlays(&layers));
    if target.audio {
        chains.extend(audio_layers(&layers).map(|(i, layer)| clip_audio(i, layer, range)));
        chains.push(mix(&layers, range));
    }
    Graph {
        inputs,
        filter: chains.join(";"),
    }
}

/// Clips that contribute to the range, in draw order: lower video tracks first,
/// later-starting clips above earlier ones on the same track.
fn layers<'a>(
    project: &'a Project,
    infos: &MediaInfos,
    target: &Target,
    range: Span,
) -> Vec<Layer<'a>> {
    let mut layers: Vec<Layer> = project
        .clips
        .iter()
        .filter_map(|clip| {
            let visible = clip.span().intersect(range)?;
            let (input, video, audio) = match clip.source() {
                Some(id) => {
                    let info = infos.get(&id)?;
                    let path = &project.source(id)?.path;
                    let video = clip.track.is_video() && info.video.is_some();
                    let audio = target.audio
                        && info.has_audio
                        && !clip.audio.muted
                        && clip.audio.volume > 0.0;
                    (Some(Input { index: 0, path }), video, audio)
                }
                None => (None, clip.track.is_video(), false),
            };
            (video || audio).then(|| Layer {
                clip,
                input,
                visible,
                input_end: clip.end().min(range.end + target.input_lookahead),
                video,
                audio,
                overlap_before: project.overlap_before(clip.id),
                overlap_after: project.overlap_after(clip.id),
            })
        })
        .collect();
    layers.sort_by(|a, b| {
        a.clip
            .track
            .cmp(&b.clip.track)
            .then(a.clip.start.total_cmp(&b.clip.start))
            .then(a.clip.id.cmp(&b.clip.id))
    });
    for (index, input) in layers
        .iter_mut()
        .filter_map(|l| l.input.as_mut())
        .enumerate()
    {
        input.index = index;
    }
    layers
}

/// Layers with their input index, which names their chains.
fn video_layers<'a, 'b>(layers: &'a [Layer<'b>]) -> impl Iterator<Item = (usize, &'a Layer<'b>)> {
    layers.iter().enumerate().filter(|(_, layer)| layer.video)
}

fn audio_layers<'a, 'b>(layers: &'a [Layer<'b>]) -> impl Iterator<Item = (usize, &'a Layer<'b>)> {
    layers.iter().enumerate().filter(|(_, layer)| layer.audio)
}

fn input_args(layer: &Layer, rate: FrameRate) -> Option<Vec<String>> {
    let t0 = layer.visible.start;
    let input = layer.input.as_ref()?;
    let path = input.path.to_string_lossy().into_owned();
    let args = if layer.clip.is_still() {
        // A picture repeats for the visible part; there is nothing to seek in.
        let len = secs(layer.visible.len());
        [
            "-loop",
            "1",
            "-framerate",
            &rate.to_string(),
            "-t",
            &len,
            "-i",
            &path,
        ]
        .map(String::from)
        .to_vec()
    } else {
        let source_t0 = secs(layer.clip.timeline_to_source(t0).max(0.0));
        let len = secs(layer.input_end - t0);
        ["-ss", &source_t0, "-t", &len, "-i", &path]
            .map(String::from)
            .to_vec()
    };
    Some(args)
}

/// Black canvas whose timestamps are absolute timeline time, like every clip's.
fn base_video(target: &Target, rate: FrameRate, range: Span) -> String {
    format!(
        "color=c=black:s={w}x{h}:r={rate},trim=end_frame={n},setpts=PTS+round({a}/TB)[base]",
        w = target.width,
        h = target.height,
        n = rate.frames(range),
        a = secs(range.start),
    )
}

fn clip_video(index: usize, layer: &Layer, target: &Target, rate: FrameRate) -> String {
    let head = match layer.clip.text() {
        Some(text) => text_source(layer, text, target, rate),
        None => media_head(layer, target, rate),
    };
    video_tail(index, layer, &head)
}

/// Shared by every video kind: scale (media only), alpha format, opacity, fades, label.
/// Position is applied by `overlays`.
fn video_tail(index: usize, layer: &Layer, head: &str) -> String {
    let clip = layer.clip;
    let Transform { scale, opacity, .. } = clip.transform;
    let scale = if clip.scalable() && scale != 100.0 {
        format!(",scale=iw*{scale}/100:ih*{scale}/100")
    } else {
        String::new()
    };
    let opacity = if opacity == 100.0 {
        String::new()
    } else {
        format!(",colorchannelmixer=aa={opacity}/100")
    };
    let fade_in = clip.video.fade_in.max(layer.overlap_before);
    let fades = fades("fade", ":alpha=1", clip, fade_in, clip.video.fade_out);
    format!("{head}{scale},format=yuva420p{opacity}{fades}[v{index}]")
}

/// Input frames stamped at the clip's timeline position and fit into the frame.
fn media_head(layer: &Layer, target: &Target, rate: FrameRate) -> String {
    let input = layer.input.as_ref().map_or(0, |i| i.index);
    let (w, h, t0) = (target.width, target.height, secs(layer.visible.start));
    // Input-seeked frames are stamped relative to the seek point, keeping the sub-frame offset
    // of the first frame; fps start_time then pads the clip's first slot instead of leaving a gap.
    format!(
        "[{input}:v:0]setpts=PTS+round({t0}/TB),fps={rate}:start_time={t0},\
             scale={w}:{h}:force_original_aspect_ratio=decrease:force_divisible_by=2"
    )
}

/// Transparent canvas lasting the visible part of the clip, stamped at its timeline position.
fn text_source(layer: &Layer, text: &TextProps, target: &Target, rate: FrameRate) -> String {
    let transform = layer.clip.transform;
    let fontsize = text.size * f64::from(target.height) / POINTS_PER_FRAME;
    let outline = if text.outline {
        format!(
            ":borderw={}:bordercolor={}",
            (fontsize / 16.0).round().max(1.0),
            font_color(&text.outline_color)
        )
    } else {
        String::new()
    };
    format!(
        "color=c=black@0:s={w}x{h}:r={rate}:d={len},format=rgba,\
         drawtext={font}:expansion=none:text={content}:fontsize={fontsize}:\
         fontcolor={color}{outline}:x=w*{x}/100-text_w/2:y=h*{y}/100-text_h/2,\
         setpts=PTS+round({t0}/TB)",
        w = target.width,
        h = target.height,
        len = secs(layer.visible.len()),
        font = font_option(),
        content = escape_text(&text.text),
        color = font_color(&text.color),
        x = transform.x,
        y = transform.y,
        t0 = secs(layer.visible.start),
    )
}

// Fontconfig in the Windows ffmpeg builds runs without a config and rescans every installed font
// on each run, which takes 20+ s once the font files drop out of the OS file cache.
// A font file skips fontconfig entirely.
fn font_option() -> &'static str {
    static FONT: OnceLock<String> = OnceLock::new();
    FONT.get_or_init(|| {
        let windows = std::env::var_os("WINDIR").map_or_else(|| "C:/Windows".into(), PathBuf::from);
        [
            windows.join("Fonts/arial.ttf"),
            "/System/Library/Fonts/Supplemental/Arial.ttf".into(),
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(),
            "/usr/share/fonts/TTF/DejaVuSans.ttf".into(),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .map_or_else(
            || "font=Sans".to_owned(),
            |path| {
                format!(
                    "fontfile={}",
                    escape_text(&path.to_string_lossy().replace('\\', "/"))
                )
            },
        )
    })
}

/// `#rrggbbaa` to ffmpeg's `0xrrggbbaa`; anything else renders white.
fn font_color(color: &str) -> String {
    match color.strip_prefix('#') {
        Some(hex) if hex.len() == 8 && hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
            format!("0x{hex}")
        }
        _ => "white".into(),
    }
}

/// Escapes for drawtext's option parser, then for the filtergraph parser around it.
fn escape_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let option = escape(&text, &['\\', '\'', ':']);
    escape(&option, &['\\', '\'', '[', ']', ',', ';'])
}

fn escape(text: &str, special: &[char]) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn overlays(layers: &[Layer]) -> String {
    let mut chain = String::new();
    let mut below = "base".to_owned();
    for (i, layer) in video_layers(layers) {
        let position = if layer.clip.scalable() {
            let Transform { x, y, .. } = layer.clip.transform;
            format!(":x=main_w*{x}/100-overlay_w/2:y=main_h*{y}/100-overlay_h/2")
        } else {
            String::new()
        };
        chain += &format!("[{below}][v{i}]overlay=eof_action=pass{position}[ov{i}];");
        below = format!("ov{i}");
    }
    chain + &format!("[{below}]setpts=PTS-STARTPTS,format=yuv420p[v]")
}

fn clip_audio(index: usize, layer: &Layer, range: Span) -> String {
    let clip = layer.clip;
    let audio = &clip.audio;
    let fade_in = audio.fade_in.max(layer.overlap_before);
    let fade_out = audio.fade_out.max(layer.overlap_after);
    let volume = if audio.volume == 1.0 {
        String::new()
    } else {
        format!(",volume={:.6}", audio.volume)
    };
    let mutes: String = audio
        .mute_regions
        .iter()
        .filter_map(|region| {
            let timeline = Span::new(
                clip.source_to_timeline(region.start),
                clip.source_to_timeline(region.end),
            );
            timeline.intersect(layer.visible)
        })
        // enable is evaluated per audio frame, so mute edges land within ~20 ms.
        .map(|span| {
            format!(
                ",volume=0:enable='between(t,{},{})'",
                secs(span.start),
                secs(span.end)
            )
        })
        .collect();
    // amix ignores timestamps, so the clip's position has to become real leading silence.
    // async/first_pts pads streams that start after the seek point (A/V offset in the source).
    let input = layer.input.as_ref().map_or(0, |i| i.index);
    format!(
        "[{input}:a:0]aresample={SAMPLE_RATE}:async=1:first_pts=0,\
         aformat=sample_fmts=fltp:channel_layouts=stereo,\
         asetpts=PTS+round({t0}/TB){fades}{volume}{mutes},\
         asetpts=PTS-STARTPTS,adelay=delays={delay}S:all=1[a{index}]",
        t0 = secs(layer.visible.start),
        fades = fades("afade", "", clip, fade_in, fade_out),
        delay = samples(layer.visible.start - range.start),
    )
}

fn mix(layers: &[Layer], range: Span) -> String {
    let clips: String = audio_layers(layers)
        .map(|(i, _)| format!("[a{i}]"))
        .collect();
    let inputs = 1 + audio_layers(layers).count();
    format!(
        "anullsrc=r={SAMPLE_RATE}:cl=stereo,atrim=end_sample={n}[abase];\
         [abase]{clips}amix=inputs={inputs}:duration=first:normalize=0:dropout_transition=0[a]",
        n = samples(range.len()),
    )
}

/// Fade filters at absolute timeline times: ffmpeg rejects negative `st`, and partial fades
/// stay exact when the range cuts through them.
fn fades(filter: &str, extra: &str, clip: &Clip, fade_in: f64, fade_out: f64) -> String {
    let fade_in = fade_in.clamp(0.0, clip.duration);
    let fade_out = fade_out.clamp(0.0, clip.duration);
    [
        ("in", clip.start, fade_in),
        ("out", clip.end() - fade_out, fade_out),
    ]
    .into_iter()
    .filter(|&(_, _, duration)| duration > 0.0)
    .map(|(kind, start, duration)| {
        format!(
            ",{filter}=t={kind}:st={}:d={}{extra}",
            secs(start),
            secs(duration)
        )
    })
    .collect()
}

fn secs(t: f64) -> String {
    format!("{t:.6}")
}

fn samples(t: f64) -> i64 {
    (t * f64::from(SAMPLE_RATE)).round() as i64
}

impl FrameRate {
    pub fn from_fps(fps: f64) -> Self {
        let ntsc = (fps * 1.001).round();
        if (fps - fps.round()).abs() < 1e-3 {
            Self {
                num: fps.round() as u64,
                den: 1,
            }
        } else if (fps - ntsc * 1000.0 / 1001.0).abs() < 1e-3 {
            Self {
                num: ntsc as u64 * 1000,
                den: 1001,
            }
        } else {
            Self {
                num: (fps * 1000.0).round() as u64,
                den: 1000,
            }
        }
    }

    fn fps(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn time(self, frame: i64) -> f64 {
        frame as f64 / self.fps()
    }

    /// Frame whose display interval contains `t`.
    pub fn frame_containing(self, t: f64) -> i64 {
        // Tolerates `t` computed as frame / fps landing a hair before the boundary.
        (t * self.fps() + 1e-6).floor() as i64
    }

    fn snap(self, range: Span) -> Span {
        let first = (range.start * self.fps()).round() as i64;
        let last = ((range.end * self.fps()).round() as i64).max(first + 1);
        Span {
            start: self.time(first),
            end: self.time(last),
        }
    }

    fn frames(self, range: Span) -> i64 {
        (range.len() * self.fps()).round() as i64
    }
}

impl fmt::Display for FrameRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.den {
            1 => write!(f, "{}", self.num),
            den => write!(f, "{}/{den}", self.num),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;
    use crate::model::Transform;
    use crate::render::tests::Timeline;
    use crate::test_support::media_info;

    impl Graph {
        fn chain(&self, label: &str) -> &str {
            self.filter
                .split(';')
                .find(|chain| chain.starts_with(label))
                .unwrap_or_else(|| panic!("no chain {label} in {}", self.filter))
        }
    }

    /// Filters of a chain without its output label: split at commas that are neither escaped
    /// nor inside quotes.
    fn filters(chain: &str) -> Vec<&str> {
        let chain = chain.rfind('[').map_or(chain, |i| &chain[..i]);
        let (mut out, mut start, mut quoted, mut escaped) = (vec![], 0, false, false);
        for (i, c) in chain.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '\'' => quoted = !quoted,
                ',' if !quoted => {
                    out.push(&chain[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        out.push(&chain[start..]);
        out
    }

    #[test]
    fn inputs_cover_only_the_part_inside_the_range() {
        let mut tl = Timeline::new(640, 360, 30.0);
        let a = tl.clip("a.mp4", Track::Video(0), 0.0, 10.0);
        tl.edit(a).source_in = 5.0;
        tl.clip("b.mp4", Track::Video(0), 8.0, 12.0);
        let graph = tl.build(4.0, 12.0);
        assert_eq!(
            graph.inputs,
            [
                "-ss", "9.000000", "-t", "6.000000", "-i", "a.mp4", //
                "-ss", "0.000000", "-t", "4.000000", "-i", "b.mp4",
            ]
        );
        assert!(
            graph
                .chain("[0:v:0]")
                .starts_with("[0:v:0]setpts=PTS+round(4.000000/TB),fps=30:start_time=4.000000,")
        );
        assert!(
            graph
                .chain("[1:v:0]")
                .starts_with("[1:v:0]setpts=PTS+round(8.000000/TB),fps=30:start_time=8.000000,")
        );
    }

    #[test]
    fn clips_outside_range_or_without_media_info_are_skipped() {
        let mut tl = Timeline::new(640, 360, 30.0);
        tl.clip("before.mp4", Track::Video(0), 0.0, 2.0);
        tl.clip("inside.mp4", Track::Video(0), 2.0, 2.0);
        tl.clip("after.mp4", Track::Video(0), 4.0, 2.0);
        let unprobed = tl.clip("unprobed.mp4", Track::Video(1), 2.0, 2.0);
        let source = tl.project.clip(unprobed).unwrap().source().unwrap();
        tl.infos.remove(&source);
        let muted = tl.clip_with(
            "muted.wav",
            media_info(false, true),
            Track::Audio(0),
            2.0,
            2.0,
        );
        tl.edit(muted).audio.muted = true;
        let silent = tl.clip_with(
            "silent.wav",
            media_info(false, true),
            Track::Audio(0),
            2.0,
            2.0,
        );
        tl.edit(silent).audio.volume = 0.0;
        let graph = tl.build(2.0, 4.0);
        assert_eq!(graph.inputs.len(), 6);
        assert_eq!(graph.inputs[4..], ["-i", "inside.mp4"]);
    }

    #[test]
    fn video_stacks_by_track_then_start() {
        let mut tl = Timeline::new(640, 360, 30.0);
        tl.clip("v2.mp4", Track::Video(1), 0.0, 10.0);
        tl.clip("v1-late.mp4", Track::Video(0), 5.0, 5.0);
        tl.clip("v1-early.mp4", Track::Video(0), 0.0, 10.0);
        tl.clip("a1.mp4", Track::Audio(0), 0.0, 10.0);
        let graph = tl.build(0.0, 10.0);
        let paths: Vec<_> = graph
            .inputs
            .chunks(6)
            .map(|input| input[5].as_str())
            .collect();
        assert_eq!(paths, ["v1-early.mp4", "v1-late.mp4", "v2.mp4", "a1.mp4"]);
        let pos = ":x=main_w*50/100-overlay_w/2:y=main_h*50/100-overlay_h/2";
        assert!(graph.filter.contains(&format!(
            "[base][v0]overlay=eof_action=pass{pos}[ov0];\
             [ov0][v1]overlay=eof_action=pass{pos}[ov1];\
             [ov1][v2]overlay=eof_action=pass{pos}[ov2];\
             [ov2]setpts=PTS-STARTPTS,format=yuv420p[v]"
        )));
    }

    #[test]
    fn media_transform_scales_fades_and_positions() {
        let mut tl = Timeline::new(640, 360, 30.0);
        let id = tl.clip("a.mp4", Track::Video(0), 0.0, 10.0);
        let default = tl.build(0.0, 10.0);
        let default = filters(default.chain("[0:v:0]"));
        assert_eq!(default.len(), 4, "{default:?}");
        tl.edit(id).video.fade_in = 1.0;
        tl.edit(id).transform = Transform {
            x: 120.0,
            y: -10.0,
            scale: 50.0,
            opacity: 40.0,
        };
        let graph = tl.build(0.0, 10.0);
        let chain = filters(graph.chain("[0:v:0]"));
        assert_eq!(chain[3], "scale=iw*50/100:ih*50/100");
        assert_eq!(chain[4], "format=yuva420p");
        assert_eq!(chain[5], "colorchannelmixer=aa=40/100");
        assert!(chain[6].starts_with("fade=t=in"), "{chain:?}");
        assert!(graph.filter.contains(
            "overlay=eof_action=pass:x=main_w*120/100-overlay_w/2:y=main_h*-10/100-overlay_h/2[ov0]"
        ));
    }

    #[test]
    fn clip_contributes_only_the_streams_its_track_and_media_have() {
        let mut tl = Timeline::new(640, 360, 30.0);
        tl.clip("music.mp4", Track::Audio(0), 0.0, 10.0);
        tl.clip_with(
            "voice.wav",
            media_info(false, true),
            Track::Video(0),
            0.0,
            10.0,
        );
        let graph = tl.build(0.0, 10.0);
        assert!(!graph.filter.contains(":v:0]"), "{}", graph.filter);
        assert!(
            graph
                .filter
                .contains("[base]setpts=PTS-STARTPTS,format=yuv420p[v]")
        );
        assert!(graph.filter.contains("[abase][a0][a1]amix=inputs=3:"));
    }

    #[test]
    fn fades_use_absolute_timeline_time() {
        let mut tl = Timeline::new(640, 360, 30.0);
        let clip = tl.clip("a.mp4", Track::Video(0), 10.0, 10.0);
        tl.edit(clip).video.fade_in = 1.0;
        tl.edit(clip).video.fade_out = 2.0;
        tl.edit(clip).audio.fade_in = 0.5;
        tl.edit(clip).audio.fade_out = 20.0;
        let graph = tl.build(15.0, 30.0);
        let video = filters(graph.chain("[0:v:0]"));
        let fades: Vec<_> = video.iter().filter(|f| f.starts_with("fade=")).collect();
        assert_eq!(
            fades,
            [
                &"fade=t=in:st=10.000000:d=1.000000:alpha=1",
                &"fade=t=out:st=18.000000:d=2.000000:alpha=1"
            ]
        );
        assert!(
            graph.chain("[0:a:0]").contains(
                ",afade=t=in:st=10.000000:d=0.500000,afade=t=out:st=10.000000:d=10.000000,"
            )
        );
    }

    #[test]
    fn overlapping_clips_crossfade() {
        let mut tl = Timeline::new(640, 360, 30.0);
        tl.clip("a.mp4", Track::Video(0), 0.0, 10.0);
        tl.clip("b.mp4", Track::Video(0), 8.0, 10.0);
        let graph = tl.build(0.0, 18.0);
        assert!(!graph.chain("[0:v:0]").contains("fade"));
        let fades = filters(graph.chain("[1:v:0]"));
        assert!(fades.contains(&"fade=t=in:st=8.000000:d=2.000000:alpha=1"));
        assert!(
            graph
                .chain("[0:a:0]")
                .contains(",afade=t=out:st=8.000000:d=2.000000,")
        );
        assert!(
            graph
                .chain("[1:a:0]")
                .contains(",afade=t=in:st=8.000000:d=2.000000,")
        );
    }

    #[test]
    fn mute_regions_map_from_source_to_timeline() {
        let mut tl = Timeline::new(640, 360, 30.0);
        let clip = tl.clip("a.mp4", Track::Video(0), 10.0, 10.0);
        tl.edit(clip).source_in = 5.0;
        tl.edit(clip).audio.mute_regions = vec![
            Span::new(7.0, 8.0),
            Span::new(0.0, 1.0),
            Span::new(9.0, 9.5),
        ];
        let graph = tl.build(12.5, 20.0);
        assert!(graph.chain("[0:a:0]").contains(
            ",asetpts=PTS+round(12.500000/TB),\
             volume=0:enable='between(t,12.500000,13.000000)',\
             volume=0:enable='between(t,14.000000,14.500000)',\
             asetpts=PTS-STARTPTS,"
        ));
    }

    #[test]
    fn audio_is_delayed_to_its_position_in_range() {
        let mut tl = Timeline::new(640, 360, 30.0);
        let clip = tl.clip("a.mp4", Track::Video(0), 3.0, 10.0);
        tl.edit(clip).audio.volume = 0.5;
        let graph = tl.build(1.0, 10.0);
        assert_eq!(
            graph.chain("[0:a:0]"),
            "[0:a:0]aresample=48000:async=1:first_pts=0,\
             aformat=sample_fmts=fltp:channel_layouts=stereo,\
             asetpts=PTS+round(3.000000/TB),volume=0.500000,\
             asetpts=PTS-STARTPTS,adelay=delays=96000S:all=1[a0]"
        );
        assert!(graph.filter.ends_with(
            "anullsrc=r=48000:cl=stereo,atrim=end_sample=432000[abase];\
             [abase][a0]amix=inputs=2:duration=first:normalize=0:dropout_transition=0[a]"
        ));
    }

    #[test]
    fn text_size_does_not_depend_on_project_resolution() {
        let fontsize = |w, h| {
            let mut tl = Timeline::new(w, h, 30.0);
            let text = tl.project.add_text(Track::Video(0), 0.0);
            let crate::model::Content::Text { text } = &mut tl.edit(text).content else {
                unreachable!()
            };
            text.outline_color = "#ff0000ff".into();
            let graph = tl.build_sized(0.0, 5.0, 960, 540);
            let chain = graph.chain("color=c=black@0").to_owned();
            assert!(chain.contains("bordercolor=0xff0000ff"), "{chain}");
            chain
                .split("fontsize=")
                .nth(1)
                .unwrap()
                .split(':')
                .next()
                .unwrap()
                .to_owned()
        };
        assert_eq!(fontsize(1920, 1080), "32");
        assert_eq!(fontsize(1280, 720), "32");
    }

    #[test]
    fn text_layer_has_no_input_and_scales_with_the_frame() {
        let mut tl = Timeline::new(1920, 1080, 30.0);
        tl.clip("a.mp4", Track::Video(0), 0.0, 10.0);
        let text = tl.project.add_text(Track::Video(1), 2.0);
        tl.edit(text).transform.x = 20.0;
        tl.edit(text).video.fade_in = 1.0;
        tl.clip("b.mp4", Track::Audio(0), 0.0, 10.0);
        let graph = tl.build_sized(0.0, 10.0, 960, 540);
        assert_eq!(graph.inputs.iter().filter(|a| *a == "-i").count(), 2);
        let chain = graph.chain("color=c=black@0");
        assert!(
            chain.starts_with("color=c=black@0:s=960x540:r=30:d=5.000000,"),
            "{chain}"
        );
        let drawtext = format!(
            "drawtext={}:expansion=none:text=Text:fontsize=32:",
            font_option()
        );
        assert!(chain.contains(&drawtext), "{chain}");
        assert!(chain.contains("fontcolor=0xffffffff:borderw=2:bordercolor=0x000000ff:"));
        let filters = filters(chain);
        assert!(
            filters
                .iter()
                .any(|f| f.contains(":x=w*20/100-text_w/2:y=h*50/100-text_h/2"))
        );
        assert!(filters.contains(&"setpts=PTS+round(2.000000/TB)"));
        assert!(filters.contains(&"fade=t=in:st=2.000000:d=1.000000:alpha=1"));
        assert!(
            graph
                .filter
                .contains("[ov0][v1]overlay=eof_action=pass[ov1]")
        );
        assert!(graph.filter.contains("[1:a:0]aresample"));
        assert!(graph.filter.contains("[abase][a0][a2]amix=inputs=3:"));
    }

    #[test]
    fn text_is_escaped_for_drawtext_and_filtergraph() {
        assert_eq!(
            escape_text("a:b,'c'[d];e\\f\r\ng"),
            r"a\\:b\,\\\'c\\\'\[d\]\;e\\\\f
g"
        );
    }

    #[test]
    fn empty_timeline_is_black_and_silent() {
        let graph = Timeline::new(640, 360, 30.0).build(0.0, 2.0);
        assert!(graph.inputs.is_empty());
        assert_eq!(
            graph.filter,
            "color=c=black:s=640x360:r=30,trim=end_frame=60,setpts=PTS+round(0.000000/TB)[base];\
             [base]setpts=PTS-STARTPTS,format=yuv420p[v];\
             anullsrc=r=48000:cl=stereo,atrim=end_sample=96000[abase];\
             [abase]amix=inputs=1:duration=first:normalize=0:dropout_transition=0[a]"
        );
    }

    #[test]
    fn range_snaps_to_frame_grid() {
        let graph = Timeline::new(640, 360, 30.0).build(1.01, 1.99);
        assert!(graph.filter.starts_with(
            "color=c=black:s=640x360:r=30,trim=end_frame=30,setpts=PTS+round(1.000000/TB)[base];"
        ));
        let graph = Timeline::new(640, 360, 30.0).build(1.0, 1.0);
        assert!(graph.filter.contains("trim=end_frame=1,"));
    }

    #[test]
    fn frame_rates_are_exact_rationals() {
        let rate = |fps| FrameRate::from_fps(fps).to_string();
        assert_eq!(rate(60.0), "60");
        assert_eq!(rate(29.97), "30000/1001");
        assert_eq!(rate(59.94), "60000/1001");
        assert_eq!(rate(23.976), "24000/1001");
        assert_eq!(rate(12.5), "12500/1000");
    }

    #[test]
    fn frame_containing_tolerates_float_error() {
        let rate = FrameRate::from_fps(60.0);
        assert_eq!(rate.frame_containing(rate.time(61)), 61);
        assert_eq!(rate.frame_containing(1.0), 60);
        assert_eq!(rate.frame_containing(1.0166), 60);
    }
}
