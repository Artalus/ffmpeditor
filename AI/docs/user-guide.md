user guide
---

# setup
Needs ffmpeg and ffprobe in one folder.
`Options` > `Settings`: ffmpeg path, `Browse…`. `Test` verifies it can run.
If ffmpeg can't run at start, an error says so. Media that failed to load is retried after the path changes.

App settings are stored in `%APPDATA%\ffmpeditor\config\config.toml`.
If that file is unreadable, a warning shows at start, and saving settings asks before overwriting it.

# menu
## project
`File` > `New`, `Open`, `Save`, `Save as`, `Quit`. Ctrl+N, Ctrl+O, Ctrl+S, Ctrl+Shift+S.
Projects are saved as `.toml`. Media paths in it are absolute. Media missing on open is shown red with the error.
Unsaved changes: the window title shows `*`.
`Project` > `Project settings`: resolution, fps, video and audio codec and bitrate, container format.
Presets fill resolution, fps and bitrate.
Codec fields accept any ffmpeg encoder name as a string; `⏷` lists common ones.

## media
`Media` > `Import media` (Ctrl+I). Several files at once. Files that can't be read are reported.
`Media` > `Add text` (Ctrl+T). The new text clip is selected.

New clips go to the selected track, starting at the playhead.
Video and text need a video track, audio-only files an audio track. If the selected track is the other kind, V1 or A1 is used.
If the new clip would overlap a clip there, a dialog offers the nearest free track above. If there is none, it offers to create one. `Cancel` adds nothing.
Several imported files are placed one after another.
Text clips start 5 s long and can be trimmed to any length.

## export
`File` > `Export` (Ctrl+E).
Range: whole timeline or the selected range.
Output: path, `Browse…`. Extension defaults to the project format.
`ffmpeg command` shows the exact command.

The progress window shows elapsed and estimated total time. `Cancel` stops and deletes the partial file.
On failure the window shows ffmpeg's error output.
Click the file name to show it in its folder.
One export at a time. Quitting application is refused while an export runs.
Aborted export leaves a partially written file.

# workspace
## tracks
`+ Track` adds a video or audio track.
Right-click a track name > `Delete`. A track with clips asks first; its clips are deleted. Tracks above it move down.
The last video and the last audio track can't be deleted.
Trash icon: the track had clips this session and is now empty. Hover it for a tooltip.
Click a track name, empty lane space or a clip to select its track. The selected track name is highlighted.

## timeline
Wheel scrolls. Ctrl+wheel zooms at the pointer. Alt+wheel, or wheel over track names, scrolls tracks.
Ruler: click moves the playhead and clears the range. Drag selects a range; its ends snap to clip edges and 0. Drag a range edge to resize it; clicking an edge keeps the range.

### clips
Clips show thumbnails, waveform and file name. Text clips show their text.
Click a clip to select it. Click empty space to deselect. Both move the playhead.
Clicking a clip moves the playhead.

Drag to move. Video clips move between video tracks, audio clips between audio tracks. A video clip's audio moves with it.
Drag an edge to trim. Trims stop at the start and end of the source.
Moves and trims snap to clip edges, the playhead and 0.
`S` splits the selected clip at the playhead; with no selected clip under the playhead, every clip under it.
`Del` removes the selected clip.
`Home`, `End`: playhead to start, end. `Left`, `Right`: one frame back, forward.

### fades
Triangles at the top corners of a clip are fade handles. Drag to adjust. A handle tints red on hover, full red while held.
Video and audio fade separately. Fade-in plus fade-out can't exceed the clip.
Two handles at the same spot: grab the left one from its left side, the right one from its right side.
Right-click a clip > `Reset fade` > `Fade-in`, `Fade-out`, `Both`. Applies to the lane clicked, video or audio.

Clips overlapping on one track crossfade, shown by a yellow X.
The later clip fades in over the overlap; audio of the earlier clip fades out over it.
A fade shorter than the overlap still fades over the whole overlap. Fade drags snap to the overlap length.

### mute
Right-click a clip: `Mute clip`, `Unmute clip`.
`Mute selection`, `Unmute selection`: uses the ruler range. Enabled when the range overlaps the clip.
Unmuting middle part of a region splits it.
Muted audio is drawn dim; muted regions are shaded.
Drag the edge of a muted region to resize it. Dragging edge over another region merges them.
Dragging both edges together asks to remove the region; `Cancel` restores it.
Muted regions stay with the clip's content when it is moved or trimmed.

## properties
Panel right of the viewer, for the selected clip on a video track.
Text clips: text (several lines), size in pt (1/1080 of frame height, same look at any resolution), color, outline and its color.
All video-track clips: position X, Y in % of the frame (clip center), opacity %. Media clips also: scale %; 100% fits the frame, like a clip on V1.
3×3 buttons place the clip in a corner, at an edge middle or at the center, keeping it inside the frame.
`Reset` restores 50/50 %, 100% scale, 100% opacity.
Each field edit is one undo step.

## preview
The viewer shows the frame at the playhead, updated while scrubbing. There is no playback.

## status bar
Playhead time / project length, selected range, ffmpeg version or error, export progress.

## errors
Errors pop up top right. `×` dismisses one, `Dismiss all` all.

## undo
`Ctrl+Z`. Redo: `Ctrl+Y` or `Ctrl+Shift+Z`. Covers every edit, 500 steps.
