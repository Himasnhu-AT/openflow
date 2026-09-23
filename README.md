# JezFlow

Personal, fully local voice dictation for macOS (a Wispr Flow replacement), built with Tauri (Rust + React + Vite). Hold **⌘;**, talk, release,
and the cleaned-up text is pasted wherever your cursor is. No cloud, no accounts, no API keys.

## How it works

1. **Hotkey** – a global event tap catches ⌘; (hold to dictate, quick tap to toggle, Esc cancels).
2. **Transcribe** – NVIDIA Parakeet TDT 0.6B on CoreML via [FluidAudio](https://github.com/FluidInference/FluidAudio). Models download once (~600 MB) into `~/Library/Application Support/FluidAudio`.
3. **Polish** – a local `llama-server` (Homebrew `llama.cpp`, Metal) running Qwen3.5-4B removes filler words,
   fixes grammar and applies the tone for the app you're in. "Verbatim" tone skips this step.
4. **Paste** – text goes on the clipboard, ⌘V is sent to the frontmost app, the old clipboard is restored.

## Requirements

- Apple Silicon, macOS 15+. Built with Rust (stable), Node + pnpm, and the Swift command line tools for the
  speech helper; Xcode is not needed.
- `brew install llama.cpp`
- A GGUF model in `~/Library/Application Support/JezFlow/models/` (default `Qwen3.5-4B-Q4_K_M.gguf`).

## Build & install

```sh
pnpm install
./scripts/build.sh --install   # builds out/JezFlow.app and copies it to /Applications
open /Applications/JezFlow.app
```

First launch: grant **Microphone** and **Accessibility** (System Settings → Privacy & Security). The
Accessibility grant is tied to the binary's ad-hoc signature, so after a rebuild you may need to remove and
re-add JezFlow in that list. Add it to Login Items to have it always running.

## Configure

JezFlow is a normal Dock app with a menu bar icon as well. The window opens at launch (Settings › Open window at
launch turns that off for login starts); clicking the Dock icon or the menu bar icon → **Open JezFlow…** brings it back:

- **General** – engine status, hotkey reference, tap threshold, Parakeet v2/v3, launch at login.
- **Models** – pick the active polish model, download Qwen3.5 2B / 4B / 9B, or drop any `.gguf` into the models folder.
  Switching restarts the local `llama-server`.
- **Tones** – edit the instruction behind each tone or add your own. An empty instruction means verbatim (no model call).
- **App Tones** – per-app tone and alternate tone (⌘⇧;). Pick from running apps or type a bundle id.
- **Dictionary** – names and jargon, one per line as `Term` or `Term: sound-alike, sound-alike`
  (e.g. `Zentro: zentrow, zen tro`). Sound-alikes are replaced by rules in every tone, the term is boosted inside the
  speech model, and the polish model sees the list.
- **Memory** – background about you that the model always sees (who you are, projects, spelling conventions).
- **Meetings** – notes written automatically (summary, decisions, action items, your to-dos as checkboxes), the live
  transcript and your own notes per meeting; a search box finds meetings by title, notes, transcript or speaker with
  live results; export as Markdown.
- **History** (Home) – every dictation with copy and a pencil to save a *correction*: what it should have said.
  Corrections are shown to the polish model as examples of your style, and feed the eval below.
- **Settings › While dictating** – floating pill (waveform while listening, then "Transcribing"), soft start/stop
  sounds, and *read the field you're typing in*: the window title and the text around the cursor are read through
  Accessibility and given to the polish model as context only, so names and mid-sentence continuations come out right.
  A leading space is added automatically when the cursor sits right after a word.
- **Settings › Tuning** – keep audio clips (on by default; 16 kHz WAVs in `clips/`, nothing leaves the Mac).

## Live text and latency

While you hold the hotkey the audio so far is re-decoded every ~350 ms (Parakeet does 15 s in ~150 ms on an M4), so
the transcript is complete the moment you release. Where the field can be read back through Accessibility (Notes,
Mail, TextEdit, most native apps) the words appear in the field as you talk and are swapped for the final text in
one exact operation; elsewhere (browsers, Electron apps, terminals) the live words show in the pill and only the
final text is pasted, once (Settings › Live text).
Past 8 s the oldest part is frozen at a pause so each re-decode stays short. On release the polished text is streamed into the field token by token,
replacing the live text as it goes, so the first polished words show up ~0.4 s after release.

What the polish step costs on an M4 (4B model, Q4_K_M): ~26 tokens/s generation, ~250 tokens/s prompt processing.
Three things keep it small:
- **Prompt caching.** Qwen3.5 is a hybrid (linear-attention) model, so llama-server can only resume from a checkpoint
  at the end of the system prompt. The system prompt is therefore byte-identical across dictations (rules, all tones,
  dictionary, memory, corrections), and the tone, field context and transcript travel in the user message as
  `[tone: casual | window: … | before: "…"]` + transcript. A tone switch costs ~60 prompt tokens, not ~400.
- **Casual fast path.** A casual message of ≤ 12 words with no correction words is finished by rules alone (lowercase,
  no punctuation except ? and apostrophes): 0 ms.
- **n-gram speculative decoding** (`--spec-type ngram-mod`) helps when the output repeats earlier text; a separate draft
  model (Qwen3.5-0.8B) was measured and is slower on this machine, so it is not used.

Per-stage timings are logged: `log stream --predicate 'subsystem == "com.jez.flow"' --level info | grep -E "Timing|LLM"`.
`JezFlow --polishtest "raw words"` replays the polish step with your settings and prints where the time went.

## Meeting notes

When a meeting app (Zoom, Teams, FaceTime, Slack, Discord, Webex, …) or a call tab in a browser is using the
microphone, a card at the top right offers **Take notes** (one click; nothing starts on its own). The menu bar item
**Start Meeting Notes** works any time. Your mic is transcribed as *You* and what comes out of the speakers as *Them*
(a Core Audio process tap; macOS asks for **System Audio Recording** once). Lines appear as each person finishes a
sentence. The speech model for meetings is SenseVoice (50+ languages, auto-detected) or Parakeet for English only
(Settings › Meetings). When you stop, Qwen writes Summary / Decisions / Action items; Refresh re-runs it. Cantonese and Mandarin come out
of the speech model in Simplified characters; Settings › Meetings › Chinese script converts transcripts, summaries
and dictation to Traditional (default). While a meeting is being recorded the bottom pill shows "Taking notes" with a
timer and a Stop button; a detected call shows "Meeting in <app> · Start taking notes" in the same pill. Meetings live
in `meetings/*.json` (transcript only, no audio) and can be exported as Markdown from the Meetings page.

## Engines: local, bigger local, or cloud

- **Dictation**: Parakeet (live text and final transcript) + Qwen3.5 4B for cleanup, all local.
- **Meetings, your side**: Parakeet. **Far side**: Whisper large v3 turbo through `whisper-server` (Homebrew
  `whisper-cpp`; model from the Models page), started for the meeting and stopped after. Any language, and much better
  than Parakeet on compressed call audio. Falls back to Parakeet when it isn't installed.
- **Meeting notes**: a bigger local model (Settings › Meetings › Notes model, e.g. Qwen3.5 9B) loaded only while the
  notes are written, so dictation keeps the fast 4B.
- **Cloud (optional)**: Settings › Processing › *Use cloud models (Groq)* with an API key from console.groq.com sends
  final transcripts (Whisper large v3 turbo), cleanup and notes (Qwen 3.8 27B, ~0.4 s) to Groq. Dictation can be
  routed for long dictations only (over 15 s), which is the default. Live text stays local, and
  any cloud failure falls back to the local models. Nothing is sent while the toggle is off or the key is empty.

## Speech-layer dictionary

Dictionary terms are also boosted inside Parakeet itself: a small CTC model (`parakeet-ctc-110m`, downloaded once)
spots the terms in the audio and the transcript is rescored, so "Jess Hub" becomes "Acme Hub" before the polish
model sees it. Terms need at least 3 characters.

## Eval: replay your own dictations

```sh
/Applications/JezFlow.app/Contents/MacOS/JezFlow --eval            # replay corrected clips through the current build
/Applications/JezFlow.app/Contents/MacOS/JezFlow --eval --no-boost # same, without speech-layer dictionary boosting
/Applications/JezFlow.app/Contents/MacOS/JezFlow --selftest        # rule-layer unit checks, no models needed
/Applications/JezFlow.app/Contents/MacOS/JezFlow --context         # what the field reader sees for the frontmost app
/Applications/JezFlow.app/Contents/MacOS/JezFlow --livetest x.wav  # push a 16 kHz wav through the live-typing path into TextEdit
```

`--eval` takes every dictation that has both a clip and a correction, runs it through the current engines, rules and
prompts, and prints word error rate against the correction: *heard* (raw transcript), *shipped* (what was pasted at
the time) and *now*. Run it before and after any prompt, rule or model change.

Everything is stored in `~/Library/Application Support/JezFlow/` (`config.json`, `history.json`, `clips/`); edits in the window
apply immediately. `open -a JezFlow --args --settings Models` opens straight to a tab.

## Logs

```sh
log stream --predicate 'subsystem == "com.jez.flow"' --level info
```

## Development

```sh
pnpm install
(cd sidecar/jezflow-helper && swift build -c release)   # speech helper (FluidAudio), optional for UI work
pnpm tauri dev                                            # app with hot reload
pnpm dev                                                  # UI only, in a browser, with mock data
cargo test --workspace                                    # rules, data, engines, platform tests
JEZFLOW_DATA_DIR=/tmp/jezflow-test pnpm tauri dev         # run against a scratch data folder
```

Layout: `src/` React UI, `src-tauri/` app shell and orchestrator, `crates/jez-core` (rules, pipeline, data),
`crates/jez-engines` (llama-server, Groq, whisper-server, speech helper client), `crates/jez-platform` (macOS
integration), `sidecar/jezflow-helper` (Swift: Parakeet, SenseVoice, system audio tap, Contacts/Calendar).
See `docs/ARCHITECTURE.md`. The original Swift app is kept locally in `dep/` (not tracked) as the reference.
