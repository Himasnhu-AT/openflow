# JezFlow

Local voice dictation and meeting notes for macOS, built with Tauri (Rust + React + Vite).
Hold **⌘;**, talk, release, and the cleaned-up text is pasted where your cursor is.

Work in progress: this is a port of the original Swift app (kept locally in `dep/`, not tracked).
See `docs/ARCHITECTURE.md` for the design and `docs/spec/` for the behavior being matched.

## Develop

```sh
pnpm install
pnpm tauri dev
cargo test --workspace
```
