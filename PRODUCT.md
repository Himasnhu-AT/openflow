# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(This record serves the marketing website in `website/`. The product itself is a native macOS desktop app built with Tauri; its own UI lives in `src/`.)

## Users
- **Primary:** Mac professionals who write all day, such as founders, engineers and writers. They want Wispr-Flow-class dictation and meeting notes without sending audio or text to a cloud service.
- **Secondary:** developers and tinkerers who build from source, read the prompts and rules, and swap models. They evaluate the product through how it works under the hood.

## Product Purpose
OpenFlow is voice dictation and meeting notes that run on your own Mac.
- **Dictation:** hold ⌘;, speak, and release. The cleaned-up text is pasted at the cursor, with fillers removed, grammar fixed and the tone matched to the app you're in.
- **Meeting notes:** it takes notes on calls without a bot joining: you and the other side are transcribed separately, then it writes a summary, decisions, action items and your to-dos.
- **Success:** people replace a cloud dictation subscription with it and keep using it every day.

## Positioning
Everything runs locally by default:
- Speech recognition: NVIDIA Parakeet on CoreML through a Swift helper.
- Cleanup: Qwen3.5 through llama.cpp.
- Audio is never stored for meetings. Nothing leaves the machine unless the user opts in to cloud models with their own Groq key, and any cloud failure falls back to local.
- It is open source (BSD 3-Clause). Every rule, prompt and model is visible and swappable.
- An `--eval` command replays the user's own corrected dictations to measure whether a change helped.

## Operating Context
The app lives in the macOS menu bar and Dock and works in any app with a text field.
- **Tones:** per-app tones. Defaults are supercasual in Messages, WhatsApp and Slack; professional in Mail, Chrome and Safari; verbatim in terminals and VS Code. Users can edit them and add their own.
- **Personalisation:** a dictionary for names and jargon, with sound-alikes; a memory about the speaker; and corrections that teach the model the user's style.
- **Live text:** words appear while speaking, either in the field or in a floating pill. The polished text replaces them on release.
- **Meetings:** call detection covers Zoom, Teams, FaceTime, Slack, Discord, Webex and browser call tabs, among others. It never starts on its own. Notes can be exported as Markdown.

## Capabilities and Constraints
- **Platform:** macOS 15+ on Apple Silicon today. Windows and Linux are planned, and the platform layer is built for them, but they are not shipped.
- **Requirements:** `brew install llama.cpp` for cleanup. Models download once: Parakeet (~600 MB) and Qwen3.5 2B, 4B or 9B from the Models page. whisper-cpp is optional, for the other side of calls.
- **Dictation features:**
  - hold to talk, quick-tap to toggle, Esc to cancel
  - ⌘⇧; for the alternate tone
  - spoken commands (new line, bullet point, emails, URLs, numbers, emoji)
  - self-corrections ("no wait, Thursday")
- **Meeting features:** Parakeet for your side; Whisper large v3 turbo or SenseVoice for the other side, covering 50+ languages; Traditional or Simplified Chinese output.
- **Optional cloud:** Groq (Whisper large v3 turbo; Qwen 3.8 27B for cleanup and notes), with the user's own key.
- **Download:** the Download for Mac button points to a release URL that does not exist yet. It is a placeholder the owner must replace.

## Brand Commitments
- **Name:** OpenFlow. Bundle id `com.himanshuat.openflow`.
- **Pitch** (from README): "Talk anywhere on your Mac. Your words land where your cursor is — cleaned up, in your voice, and they never leave your machine."
- **Hotkey:** ⌘; is part of the identity.
- **License:** BSD 3-Clause. Say "open source".

## Evidence on Hand
- **Measured on this Mac (Apple Silicon):**
  - ~70 ms to transcribe a 3.8 s clip (steady state), 160–200 ms with dictionary boosting
  - 0.6 ms of rule processing per 60-word dictation
  - 110/110 self-tests passing
  - a system prompt byte-identical to the original implementation
- **Recorded by the original Swift app on an M4:** ~0.7 s polish with the 4B model; ~0.4 s for the first polished words after release.
- **Screenshots:** real app screenshots can be captured from the running app, using synthetic sample data labeled as such.
- **Absent (must not be fabricated):** testimonials, user counts, customer logos, press, pricing and download numbers.

## Product Principles
1. Private by default. Local is the path; cloud is an opt-in convenience that always falls back.
2. It writes like you. It keeps the speaker's words and fixes only what dictation broke.
3. Speed is a feature. Text should feel instant on release.
4. Nothing is hidden. Rules, prompts and models are inspectable, and improvements are measured on your own voice.

## Accessibility & Inclusion
Voice input is itself an accessibility aid, which matters to people with RSI or limited typing. The site must meet WCAG 2.1 AA and respect reduced motion.
