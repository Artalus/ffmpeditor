# ci/cd
github actions, `.github/workflows/`.
- `ci.yml`: on pull requests, and called by `release.yml`. job `prek`: `prek run --all-files` (pre-commit stage: fmt, clippy-fix -D warnings, actionlint, whitespace) on linux. job `test`: `cargo test` on windows, linux, macos with real ffmpeg on PATH.
- `release.yml`: on push to main. runs `ci.yml`; if green, builds `--profile minsizerel` on the three OSes and uploads the binaries (`.exe` windows, `.tar.gz` else, keeps the exec bit) to the draft release for `v<Cargo.toml version>`, created if missing, assets replaced if draft. published release with that tag: warning only; bump version for a new draft.
- linux needs no `-dev` packages: wayland, xkbcommon, x11, egl, fontconfig are all dlopen'd at runtime. tests need `fonts-dejavu-core` for drawtext.
