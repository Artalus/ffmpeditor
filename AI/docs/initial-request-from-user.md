simple video editor akin to vegas.
want it designed as minimalistic gui around ffmpeg binary.
target audience/flow: concat, cut, apply minimal effects to gaming recordings.

requested functionality:
- settings section where can select ffmpeg location (store in configuration file)
- new project
- select one or more videos
- display tracks for:
  - video clips with small preview frames
  - audio clips with waveform
  - "special effects" like text overlays, image overlays, other video overlays
- video clips can be dragged around, their audio follows them
- audio can be muted on entire clip or on selected region
- can add extra audio files and drag them around independently
- audio and video support fadein, fadeout, crossfade if overlap
- save project to file in simple human-readable format (json, toml, yaml, or invent your own)
- open project from file
- project settings (final resolution, fps, bitrate, file format)
- select fragment or entire timeline to be exported in desired settings to filename


language: rust
platform: windows primary, crossplatform secondary. strive for compatibility but can ditch it *IN CORNER CASES* for simpler design.

* ffmpeg binary available at: /c/Users/Artalus/scoop/shims/ffmpeg
