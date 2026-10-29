# architecture

egui/eframe gui around ffmpeg/ffprobe binaries. no ffmpeg linking; everything is spawned processes.

## modules
- `model` — project data: settings, sources, clips; pure edit ops (append, move, split, trim, overlaps). saved as TOML.
- `config` — app config (ffmpeg path) in OS config dir (`%APPDATA%\ffmpeditor\config\config.toml`).
- `ffmpeg` — binary locations, `Command` builders (no console window on windows).
- `media` — ffprobe → `MediaInfo`; background thumbnail + waveform extraction; `MediaCache` holds results/textures.
- `render` — project → ffmpeg filter graph. export job with progress/cancel. previewer renders still frame at playhead.
- `ui::timeline` — timeline widget: ruler, tracks, clips, drag/trim, playhead, range selection.
- `app` — window shell: menus, dialogs, shortcuts, undo, wiring.

## model
- time = `f64` seconds everywhere.
- clip on video track `V<n>` brings video + its own audio (audio follows video). clip on audio track `A<n>` brings audio only.
- higher video track draws on top.
- clips overlapping on same track = crossfade; later clip fades in over the overlap.
- mute regions stored in source time, stick to content when clip moves/trims.
- media info (duration, size, fps) is not saved; re-probed on open.

## render
- one ffmpeg input per clip, input-seeked (`-ss`/`-t`), so cost scales with rendered range, not source length.
- video: black canvas, clips overlaid in track/start order; fades are alpha fades (fade from black or crossfade over clip below).
- audio: per-clip fades/mutes/volume, delayed to position, `amix` with silence base.
- fades evaluated in absolute timeline time, so exporting a sub-range keeps partial fades exact.
- preview frame = same graph over a one-frame range, PNG to stdout.

## project file example
```toml
[settings]
width = 1920
height = 1080
fps = 60.0
video_codec = "libx264"
video_bitrate_kbps = 12000
audio_codec = "aac"
audio_bitrate_kbps = 192
format = "mp4"

[[sources]]
id = 1
path = "D:\\rec\\match1.mp4"

[[clips]]
id = 1
source = 1
track = "V1"
start = 0.0
source_in = 12.5
duration = 30.0

[clips.audio]
muted = false
volume = 1.0
fade_in = 0.0
fade_out = 0.5

[[clips.audio.mute_regions]]
start = 20.0
end = 22.0
```
