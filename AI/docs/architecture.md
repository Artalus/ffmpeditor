# architecture

egui/eframe gui around ffmpeg/ffprobe binaries. no ffmpeg linking; everything is spawned processes.

## modules
- `model` — project data: settings, sources, clips, track counts; pure edit ops. saved as TOML.
- `config` — app config (ffmpeg path) in OS config dir (`%APPDATA%\ffmpeditor\config\config.toml`).
- `ffmpeg` — binary locations, `Command` builders (no console window on windows), stderr/failure helpers, `Slot` for killable processes.
- `media` — ffprobe → `MediaInfo`; background thumbnail + waveform extraction; `MediaCache` holds results/textures.
- `render` — project → ffmpeg filter graph. export job with progress/cancel. previewer renders still frame at playhead.
- `ui::timeline` — timeline widget. `layout` geometry, `edit` hit-testing/snapping/drag math, `paint` drawing.
- `ui` other — viewer, status bar, dialogs, project settings, export dialog.
- `app` — window shell: menus, shortcuts, undo, file ops, export wiring.
- `test_support` — tests only: real-ffmpeg runner, cached media fixtures under `target/test-fixtures`, polling helper.

## model
- time = `f64` seconds everywhere.
- track counts (`tracks.video`, `tracks.audio`) are stored, min 1 each. load raises them to cover clips. removing a track drops its clips and renumbers higher tracks of that kind.
- clip on video track `V<n>` brings video + its own audio (audio follows video). clip on audio track `A<n>` brings audio only.
- higher video track draws on top.
- clips overlapping on same track = crossfade.
- fades stored per clip, separately for video and audio, seconds, `fade_in + fade_out <= duration`.
- render applies effective fades: video fade-in and audio fade-in = max(stored, overlap with earlier clip); audio fade-out = max(stored, overlap with later clip); video fade-out = stored (later clip fades in over it).
- mute: whole clip flag, plus regions in source time, kept sorted and merged; regions stick to content when clip moves/trims.
- media info (duration, size, fps) is not saved; re-probed on open.
- clip content: media (`source`) or text (`[clips.text]`: `text`, `size` in pt (1/1080 of frame height), `color` `#rrggbbaa`, `outline` bool, `outline_color` `#rrggbbaa`). a clip has exactly one. text clips are video only and have no length limit; trims are free.
- `[clips.transform]`: `x`, `y` = clip center in % of frame (default 50, 50). used by text clips.
- placement of new clips: start = playhead, track = selected track if its kind fits, else V1/A1. if the span overlaps a clip there: nearest track above of the same kind that is free over the span, else a new track; the app confirms either choice.

## timeline
- rows: per video track a video lane + its linked audio lane; then one lane per audio track.
- hit priority inside a lane: fade handle (top 8 px band) > trim edge (6 px) > mute region edge (audio lanes) > clip body.
- fade handle sits where its fade ends; grabbed only on the side its triangle extends to, so stacked handles stay reachable.
- snapping 8 px: clip edges, playhead, 0. fade drags also snap to the crossfade overlap.
- draw order per lane: clip bodies with media and own fade shade; fade lines, names, borders; crossfade X; handles.
- trash hint: track had clips this session, is empty, is not the last of its kind.
- selected track: set by clicking a track name, empty lane space or a clip; runtime only, not saved.
- text clips: own body color, text as label, no thumbnails, empty linked audio lane.
- edits mutate the project directly; app takes an undo snapshot when no drag is in progress.

## app
- undo = whole-project snapshots, 500 steps.
- shortcuts ignored while a dialog is open, a text field has focus, or a timeline drag runs. plain-key shortcuts require no modifiers.
- `Media` menu: import through the placement rule. imported files are grouped by track kind; each group is placed back to back as one span, with at most one confirmation per group. add text: a 5 s text clip through the same rule, then selected.
- properties panel (right of viewer) edits the selected text clip. a focused panel field counts as an ongoing edit, so the undo snapshot is taken when it loses focus.
- unreadable config: warning at start, confirmation before it is overwritten.

## render
- one ffmpeg input per clip, input-seeked (`-ss`/`-t`), so cost scales with rendered range, not source length.
- range is snapped to the project frame grid; fps is an exact rational (`30000/1001`), so NTSC stays on ffmpeg's own grid.
- video: black canvas, clips overlaid in track/start order; fades are alpha fades (fade from black or crossfade over clip below).
- `fps=..:start_time` pads a clip's first slot with its first source frame; without it a cut flashes black.
- text: no input; transparent `color` source of frame size + `drawtext` (`fontfile` = Arial on windows/mac, DejaVu Sans on linux, `font=Sans` if none found; not fontconfig lookup, which in windows builds has no config and rescans all fonts every run, 20+ s cold; `fontsize` = pt × frame height / 1080, `fontcolor`, `borderw` + `bordercolor` (outline color) when outlined, `expansion=none`), centered on the transform point; then fades and overlay like any video layer.
- audio: per-clip fades/mutes/volume, `aresample async` aligns streams starting after the seek point, `adelay` places the clip sample-exact, `amix` with silence base.
- fades evaluated in absolute timeline time, so exporting a sub-range keeps partial fades exact.
- preview frame = same graph over a one-frame range, built at reduced size (fits 960x540), PNG to stdout.
- previewer: newest request wins, a request with the same args as the last is skipped; ffmpeg in flight is killed on drop.
- graphs too long for the command line go through `-/filter_complex <file>`.
- cancelling an export kills ffmpeg and deletes the partial output.

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

[clips.video]
fade_in = 1.0
fade_out = 0.0

[clips.audio]
muted = false
volume = 1.0
fade_in = 0.0
fade_out = 0.5

[[clips.audio.mute_regions]]
start = 20.0
end = 22.0

[tracks]
video = 2
audio = 1
```
