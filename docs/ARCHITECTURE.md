# OpenFlow (Tauri) architecture

Rust + React/Vite port of the Swift app, which is kept in `dep/` (git-ignored, reference only).
Behavioral specs are in `docs/spec/{dictation,meetings,ui}.md`. **Parity with the Swift app is the goal.** When a spec and the Swift source (`dep/Sources/JezFlow/*.swift`, absolute path `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`) disagree, the Swift source wins.

## Layout

```
src/                     React UI. main.tsx = main window, overlay/main.tsx = floating pill. lib/api.ts = IPC contract.
src-tauri/               Tauri app: orchestrator (state machine), commands, tray, windows, CLI tools.
crates/openflow-core/         Pure logic, no OS or network: config, model (DTOs), store, text_rules, pipeline,
                         meeting, segmenter, catalog, wav, wer, whisper_filter, traits (all cross-crate contracts).
crates/openflow-engines/      Processes and network: ollama, polisher (Ollama or llama-server), prompts, cloud (Groq), whisper_server,
                         notes, meeting_engine, downloader, sidecar (Swift helper client), live (live re-decode).
crates/openflow-platform/     OS integration behind openflow_core::traits: macos/ (hotkey, AX, keyboard/clipboard, mic,
                         devices, apps, sounds) + cross-platform live_typer and detector built on the traits.
sidecar/openflow-helper/  Swift package (FluidAudio): ASR, CTC vocabulary boosting, SenseVoice, Core Audio
                         process tap, Contacts/Calendar names. Shipped as a Tauri externalBin.
```

Dependency direction: `src-tauri → openflow-engines, openflow-platform → openflow-core`. `openflow-core` has no OS or network deps.
Windows/Linux ports add `openflow-platform/src/{windows,linux}` and a sherpa-onnx `AsrEngine` + WASAPI/PipeWire
`SystemAudio`; nothing above the traits changes.

## Data compatibility (must hold)

- Data dir: `~/Library/Application Support/OpenFlow/` (override with `OPENFLOW_DATA_DIR`). The files are
  `config.json`, `history.json`, `stats.json`, `clips/<uuid>.wav`, `meetings/<UUID>.json` and `models/`.
- JSON is byte-compatible with the Swift app:
  - camelCase keys, with the Swift spellings `cloudAPIKey`, `cloudASRModel` and `cloudLLMModel`.
  - Dates are f64 seconds since 2001-01-01 (`openflow_core::apple_date`).
  - UUIDs are uppercase strings.
  - Output is pretty-printed with sorted keys.
- Bundle identifier: `com.himanshuat.openflow`. Logs use subsystem `com.himanshuat.openflow`.

## Ports and processes

| What | Port / binary |
|---|---|
| Ollama (polish, default engine) | 127.0.0.1:11434 (`config.ollamaUrl`); started via Ollama.app or `ollama serve` |
| llama-server (polish, alternative/fallback) | 127.0.0.1:8765 (`config.llamaPort`) |
| llama-server (notes model, transient) | 127.0.0.1:8767 |
| whisper-server (meetings, far side) | 127.0.0.1:8768 |
| openflow-helper | child process, stdio |

## Sidecar protocol (`openflow-helper` ⇄ `openflow_engines::sidecar`)

Both directions use the same framing. Each frame is:
- `u32` little-endian length of the JSON header,
- the UTF-8 JSON header,
- if the header has `"audio": n`, then `n × 4` bytes of f32 little-endian samples (16 kHz mono).

Messages:
- Request: `{"id": 7, "method": "transcribe", "params": {...}, "audio": 48000}`
- Response: `{"id": 7, "result": {...}}` or `{"id": 7, "error": "message"}`
- Push (no id), sent by the helper:
  - `{"event": "tapAudio", "audio": n}` + payload
  - `{"event": "tapRestarted"}`
  - `{"event": "log", "level": "info", "message": "..."}`

Requests run concurrently on the Rust side (matched by id). The helper handles requests **serially per model**, as the Swift actor did.

| method | params | audio | result |
|---|---|---|---|
| `ping` | – | – | `{}` |
| `load` | `{version: "v2"\|"v3"}` | – | `{}` (downloads models on first run) |
| `transcribe` | `{rescore: bool}` | yes | `{text}` (trimmed; rescore = CTC vocabulary boosting when a vocabulary is set) |
| `setVocabulary` | `{terms: [String]}` | – | `{}` (terms already merged/filtered by Rust; empty = boosting off) |
| `senseVoice` | – | yes | `{text}` (loads SenseVoice lazily) |
| `tapStart` | `{bundle: ""}` | – | `{}` then `tapAudio` pushes. Error = no permission / not supported |
| `tapStop` | – | – | `{}` |
| `tapProcesses` | `{match: String}` | – | `{count}` (debug, `--tapprocs`) |
| `names` | – | – | `{names: [String]}` (Contacts + 48 h Calendar, asks permission) |

## Tauri IPC

Defined in `src/lib/api.ts` (TS) and `src-tauri/src/commands.rs` (Rust).
- `snapshot`: the full `Snapshot` state. It is also emitted as the `snapshot` event on every change.
- `set_config` replaces and saves the whole config.
- Remaining commands: dictation actions, meeting actions, models, devices and apps, and open/copy/launch-at-login.
- `overlay`: `OverlayState` for the pill window.
- `level`: mic RMS. `download`: download progress. `navigate`: page to show.

## State machine (src-tauri orchestrator)

`Loading → Idle → Recording → Processing → Idle`. Any state can move to `Error(msg)`, which auto-clears after 4–5 s.
- One tokio task owns the state.
- Hotkey events, tray clicks, commands and engine callbacks send it messages.
- It emits `snapshot` and `overlay` whenever something changes.

## Team ownership (phase 1)

| Team | Owns |
|---|---|
| A rules | `openflow-core/src/{text_rules/,pipeline.rs,wer.rs,whisper_filter.rs}` |
| B data | `openflow-core/src/{config_io.rs,store.rs,meeting.rs,segmenter.rs,catalog.rs}` |
| C engines | `openflow-engines/src/{polisher,prompts,cloud,whisper_server,notes,meeting_engine,downloader}.rs` |
| D sidecar | `sidecar/openflow-helper/`, `openflow-engines/src/{sidecar,live}.rs` |
| E platform | `openflow-platform/src/**` |
| F frontend | `src/**` (except `src/lib/api.ts`, which is shared: additive changes only, note them in the commit) |

Shared files (`Cargo.toml`s, `openflow-core/src/{lib,traits,model,config}.rs`, `api.ts`, this doc):
- Change them only when necessary.
- Make the change additive, in its own commit, with the reason in the message.
- Each crate's `Cargo.toml` `[dependencies]` belongs to the team that owns that crate's code. Teams A and B both add to openflow-core's list; keep those additions additive.
