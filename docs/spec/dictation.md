> Reference spec of the legacy Swift app (now in `dep/`, git-ignored; absolute path `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`). Where this spec and the Swift source disagree, the Swift source wins. Prompts, tone texts and regexes must be copied verbatim from the Swift source.

# JezFlow dictation path: behavioral spec for the Tauri/Rust port

All files are in `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`. Default values come from `Config.swift`.

## 0. Config defaults the dictation path uses
- `hotkeyKeyCode=41` (`;`), `tapThreshold=0.5` s, `llamaPort=8765`, `model="Qwen3.5-4B-Q4_K_M.gguf"`, `llamaServerPath=/opt/homebrew/bin/llama-server`, `asrVersion="v2"`, `microphone=""`.
- `defaultTones = {tone:"supercasual", alt:"professional"}`.
- `appTones` (bundle ID → tone/alt):
  - MobileSMS, WhatsApp, Slack → supercasual/professional
  - Mail, Chrome, Safari → professional/casual
  - Terminal, iTerm2, VSCode → verbatim/casual
- `tones`: `supercasual`, `casual` and `professional` hold instruction text. `verbatim` is `""`, and an empty instruction means the model is skipped.
- `dictionary=["JezFlow: jez flow, jazz flow, jazz flo", "no wait: no wake, no weight, know wait"]`, `learnedSignals=[]`, `memory=""`.
- Flags: `keepClips`, `overlay`, `sounds`, `fieldContext` and `liveText` are all true. `chineseScript="traditional"`.
- Cloud: `cloudEnabled=false`, `cloudDictation="long"` (`off|long|all`; a legacy bool maps true→`all`, false→`off`), `cloudMeetings=true`, `cloudASRModel="whisper-large-v3-turbo"`, `cloudLLMModel="qwen/qwen3.8-27b"` (the env var `JEZ_CLOUD_LLM` overrides it).
- `liveTextExcluded`: Terminal, iterm2, Warp-Stable, kitty, alacritty, ghostty, wezterm, hyper, VSCode.

## 1. AppDelegate.swift: orchestration
**State:** `loading | idle | recording | processing | error(String)`. Every change calls `render()`.

**Launch:**
- Request microphone access. If denied, go to `error("Microphone permission denied")`.
- Call `AXIsProcessTrustedWithOptions(prompt:true)`.
- Create `HotkeyMonitor(keyCode)` and wire `onPress(alt)`, `onRelease`, `onEscape`, and `isRecording` (true when state is recording).
- Connect `recorder.onLevel` to `overlay.level`.
- Run `loadTranscriber()` (`state=.loading` → `transcriber.load(asrVersion)` → `.idle` → `setVocabulary(dictionary)`), then `startPolisher()`.
- `startPolisher()`: start llama-server, wait until ready (120 s), then send one warm-up polish of `"warm up"`. If it fails, `polisher=nil` and dictation stays verbatim.
- Refresh Contacts/Calendar names (`store.externalNames`) at launch and every 3600 s, only if `useContacts` is on.

**`hotkeyPressed(alt)`:** if idle, `beginRecording(alt)`. If recording and `toggleMode`, `finishRecording()`. Otherwise ignore.

**`hotkeyReleased()`:** only acts while recording with `pressedAt` set.
- If `toggleMode`, return.
- If `now - pressedAt < tapThreshold` (0.5 s), set `toggleMode=true` and render. A quick tap keeps recording until the next press.
- Otherwise `finishRecording()`.

**`beginRecording(alt)`:**
1. `targetBundleID` = frontmost app bundle ID.
2. `fieldContext = config.fieldContext ? FieldContext.capture() : nil`.
3. Set `useAlt=alt`, `toggleMode=false`, `liveText=""`, then `typer.begin(context:)`.
4. `recorder.preferredDevice = config.microphone`, then `recorder.start()`. If it throws, go to `.error(msg)`.
5. Set `pressedAt=Date()` after `start()`, so start-up cost never counts as holding the key.
6. `Cue.start()` if sounds are on. Set `state=.recording`, then `startLive()`.

**`startLive()` task:**
- Build `names = TextRules.contextNames(windowTitle + " " + before + " " + after)`.
- Call `transcriber.setVocabulary(dictionary, extra: names + store.externalNames)`, then `transcriber.beginLive()`.
- Loop while not cancelled and still recording:
  - `snap = recorder.snapshot()`.
  - If `snap.count > 8000` (0.5 s), `text = transcriber.liveUpdate(snap)` and store it in `liveText`.
  - If `config.liveText` is on and state is still recording:
    - If `liveTextAllowed`, call `typer.update((needsLeadingSpace ? " " : "") + text)`.
    - Otherwise call `overlay.live(text)`.
  - Sleep `max(120, 400 - decodeMs)` ms, so the target cadence is about 400 ms including decode.
- `liveTextAllowed` = the bundle ID is not in `liveTextExcluded` AND `fieldContext.cursor != nil` (the AX cursor is readable).

**`cancelRecording()`** (Escape, only while recording):
- `recorder.stop()`, cancel the live task.
- Async: `transcriber.beginLive()`, then `typer.retract()`.
- `state=.idle`. No sound plays.

**`finishRecording()`:**
1. `samples = recorder.stop()`. Play `Cue.stop()`. `state=.processing`. `tone = resolveTone()`.
2. Cancel the live task and await it, so an in-flight decode finishes first.
3. If `samples.count <= 4000` (< 0.25 s): `typer.retract()`, `beginLive()`, idle. No error is shown.
4. If `peak = max(samples) <= 1e-5` (digital silence): retract, `beginLive`, then `error("Microphone gave silence: pick another input in Settings (built-in is off while the lid is closed)")`. After 5 s, go back to idle if still in error.
   - Note: `max` is the signed maximum, not the maximum absolute value.
5. `isLong = samples.count > 15*16000 || Pipeline.hasCorrectionSignal(liveText, learned: learnedSignals)`. Set `polisher.currentIsLong = isLong`. `cloud = Cloud(config, .dictation, long:isLong)`.
6. Run in parallel:
   - `async let localRaw = transcriber.liveFinish(samples)`.
   - If `cloud` exists, `raw = cloud.transcribe(samples)`. On error, log it and show the overlay message "Cloud unavailable, used local models".
   - Await the local result. If `raw` is empty, use `local`.
7. If `raw` is still empty: retract. If `peak < 1e-6`, show the same silence error for 5 s. Otherwise go idle.
8. `prefix = needsLeadingSpace ? " " : ""`.
9. `text = Pipeline.run(raw, tone, config, polisher, examples: store.allCorrections(), context: fieldContext)`. No `onPartial` is passed, so dictation never streams.
10. If `prefix` is set and `text` doesn't start with `"\n"`, prepend `prefix`.
11. If `typer.typed == text`, call `typer.forget()`. Otherwise call `typer.finish(with:text)`. Both remaining branches are identical.
12. Save history as `Dictation(date, text, raw, app: targetBundleID, tone, seconds = count/16000)`.
    - If `keepClips`, write `clips/<uuid>.wav` (Float32 16 kHz mono). On failure, set `clip=nil`.
    - Call `store.remember(d)`, rebuild the history menu, go idle.
13. Any thrown error: retract, `.error(msg)`, then back to idle after 4 s.

**`resolveTone()`:**
1. Return `toneOverride` if it is set.
2. Otherwise take `t = appTones[bundleID] ?? defaultTones`.
3. Return `useAlt ? t.alt : t.tone`. `useAlt` is true when Shift is held at key-down.

**Tone menu:**
- "Automatic (per app)" sets `toneOverride=nil`.
- Below a separator, one item per `tones.keys.sorted()`, with the title capitalized. Picking one sets `toneOverride=name`; the checkmark shows the active choice.
- The menu is rebuilt in `menuWillOpen`.

**History menu:**
- Shows the first 15 entries.
- Title: newlines become spaces, truncated to 70 characters plus `…`. The tooltip holds the full text.
- Items 1–9 get key equivalents `"1"`..`"9"` with no modifiers.
- Clicking an item copies its text to the clipboard and sets the status line to "Copied to clipboard".
- An empty history shows a disabled "No dictations yet". "Clear History" is at the bottom.

**`render()`** (SF symbol + status text):

| State | Symbol | Text |
|---|---|---|
| loading | hourglass | "Loading models…" |
| idle, tap not running | mic | "Grant Accessibility to enable the hotkey" |
| idle, no polisher | mic | "Ready (verbatim only — polish model not loaded)" |
| idle | mic | "Ready — hold ⌘; to dictate" |
| recording | mic.fill | toggle: "Recording (tap ⌘; to stop, Esc to cancel)"; held: "Recording (release to finish)" |
| processing | ellipsis.circle | "Transcribing…" |
| error | exclamationmark.triangle | "Error: m" |

- If `config.overlay` is off, the overlay is hidden.
- Otherwise: recording → `.listening`, processing → `.processing`, error → `.message(m)`, else `meetingLive` / `meetingOffer` / hide.

## 2. HotkeyMonitor.swift
- `CGEvent.tapCreate(.cgSessionEventTap, .headInsertEventTap, .defaultTap, mask keyDown|keyUp)`, added to the main run loop in common modes.
- If creation fails (no Accessibility), retry every 3 s. `isRunning = tap != nil`.
- `tapDisabledByTimeout` / `tapDisabledByUserInput`: re-enable the tap and pass the event through.
- Escape (keycode 53) on keyDown while `isRecording()`: dispatch `onEscape` and swallow the event (return nil).
- Any keycode other than `keyCode`: pass through.
- keyDown of `keyCode`:
  - Requires Cmd held and neither Ctrl nor Option. Otherwise pass through.
  - Autorepeat (`keyboardEventAutorepeat != 0`) is swallowed.
  - `alt = Shift held`. Dispatch `onPress(alt)` asynchronously on main and swallow.
- keyUp of `keyCode`: always dispatch `onRelease` and swallow, even if Cmd was released first.
  - Edge case: a plain `;` keyUp is swallowed too, even when nothing is recording. The port should decide whether to keep this.

## 3. AudioRecorder.swift
**Capture:** 16 kHz mono Float32.
- `AVCaptureSession` + `AVCaptureAudioDataOutput` with settings LinearPCM, 16000 Hz, 1 channel, 32-bit float, interleaved.
- Callbacks run on a serial queue `com.jez.flow.mic`.

**`start()`:**
1. `wanted = AudioDevices.resolve(preferredDevice)`.
2. Pick the first `AVCaptureDevice` from discovery (`.microphone`, `.external`) matching uid or name. Otherwise use the default. If there is none, throw "Microphone not available (is it connected and awake?)".
3. Rebuild the session only if none exists or the device UID changed. The old session is stopped on a background queue.
4. Register observers once: disconnected/connected, runtime error, interrupted, interruption ended, did-stop.
   - A disconnect of the current device clears `sessionDevice`, forcing a rebuild on the next start.
5. Cancel the pending idle-stop. Under a lock: clear samples, `capturing=true`.
6. `startRunning()` is dispatched onto the capture queue (it is never called on main). A cold Bluetooth mic can block for about 1 s, and blocking main would turn a tap into a hold.

**`snapshot()`:** returns a locked copy of the samples.

**`stop()`:** under the lock, `capturing=false` and take the samples. The session keeps running for `keepWarm=20` s (audio discarded), then `stopRunning` runs on a utility queue. This avoids AirPods' roughly 1–2 s wake-up silence.

**`shutdown()`:** stops immediately.

**Per buffer:**
- `CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer` → `[Float]`.
- If capturing, append the buffer.
- Log each second: sample count and peak.
- `onChunkRaw(chunk)`.
- `onLevel(RMS = sqrt(Σx²/n))`.

**WAV helpers:**
- `writeWav`: Float32 via AVAudioFile, creates the directory if needed.
- `readWav`: the inverse.

## 4. AudioDevices.swift
**`inputs()`:** Core Audio `kAudioHardwarePropertyDevices`. Keep devices with input channels > 0 (summed from `kAudioDevicePropertyStreamConfiguration` with input scope).
- Exclude transport types Aggregate and Virtual, and names starting with "JezFlow".
- `isBuiltIn`: transport is BuiltIn.
- `isContinuity`: transport is ContinuityCaptureWired or ContinuityCaptureWireless, or the name contains "iphone" or "ipad" (case-insensitive).

**`resolve(pref)`:**
- `"builtin"` → the first built-in device.
- A non-empty value → match by uid.
- `""` → the device with ID `kAudioHardwarePropertyDefaultInputDevice`, or else the first device. If that is built-in and the lid is closed, switch to the first device that is neither built-in nor Continuity.

**`lidClosed()`:** IOKit `IOPMrootDomain`, property `AppleClamshellState`.

## 5. FieldContext.swift
**Fields:** `windowTitle`, `before`, `after`, `cursor: Int?` (UTF-16 offset).
- `isEmpty`: all three strings are empty.
- `needsLeadingSpace`: the last character of `before` exists and is not whitespace or newline.

**`capture()`:**
1. `AXUIElementCreateApplication(frontmost pid)`.
2. `kAXFocusedWindowAttribute` → `kAXTitleAttribute`, truncated to 120 characters.
3. `kAXFocusedUIElementAttribute` → `kAXValueAttribute` (String, must be non-empty).
4. Default `cursor = utf16.count`.
5. If `kAXSelectedTextRangeAttribute` is readable as an AXValue CFRange: `cursor = clamp(location, 0, count)` and set `ctx.cursor`. If the range isn't readable, `ctx.cursor` stays nil.
6. `before` = the last 3000 UTF-16 units before the cursor. `after` = the first 600 units after it.

**`replace(location, length, expecting, with text) -> Bool`:**
1. Only runs when the frontmost bundle ID starts with `com.apple.`. Browsers and Electron misapply the edit; one inserted the text nine times.
2. Both `kAXSelectedTextRange` and `kAXSelectedText` must be settable, and `location+length <= value.utf16.count`.
3. Set the range to (location, length), then set `kAXSelectedText=text`.
4. Read back `kAXValue`. If the UTF-16 slice at `location` equals `text`, set the caret to `(location+text.len, 0)` and return true.
5. Otherwise, if the original live text is still at `location`, put the caret at the end of it, then return false.

## 6. LiveTyper.swift (live text + final swap; MainActor)
**Rules:**
- Text is only inserted by pasting (clipboard + ⌘V). It is never typed as keystrokes, because apps autocorrect typed text and backspace counts then drift.
- Removal uses backspace (keycode 51).
- Events use `CGEventSource(.combinedSessionState)` and are posted to `.cghidEventTap`, as a down/up pair with flags set.
- ⌘V is keycode 9 with `.maskCommand`.

**`begin(ctx)`:** `typed=""`, save the clipboard string, `fieldBefore = ctx.before` (nil if both `before` and `windowTitle` are empty), `cursor = ctx.cursor`, `desynced=false`.

**`update(text)`:**
1. Return immediately if `desynced`.
2. Replace `\n` with space. Strip trailing `.`, `,` and spaces. If the result equals `typed`, return.
3. `common` = length of the common character prefix of `typed` and `next`.
4. `keep = min(max(common, typed.count - 80), next.count)`. `tailWindow=80`: only the last 80 characters are ever rewritten.
5. Backspace `typed.count - keep` times. Paste `next.dropFirst(keep)`. Set `typed` accordingly.
6. If a cursor is known: poll `FieldContext.capture().cursor >= start + typed.utf16.count` up to 6 times at 100 ms intervals. If the paste never lands, set `desynced=true`, which stops live typing for this dictation.
7. If no cursor is known, sleep 120 ms.

**`measuredLive()`:**
- Requires `cursor`, and a recapture with `cur >= start`.
- `cur - start <= typed.utf16 + max(8, typed.utf16/5)`.
- `live` = the last `cur - start` UTF-16 units of `before`.
- Returns `(start, len, live)`.

**`retract()`:** backspace `measuredLive()?.text.count ?? typed.count`, sleep 1 s, restore the clipboard.

**`finish(text)`:**
1. `toRemove = typed.count`.
2. If `measuredLive()` returns a value: `toRemove = m.text.count`. Try `FieldContext.replace(...)`; on success, restore the clipboard and return.
3. Else if `desynced`: `toRemove=0`, so the final text is appended after the live text.
4. Backspace `toRemove` times, then paste `text`.
5. Clipboard restore timing:
   - `probe` = the first 30 characters of the trimmed text.
   - `deadline = now + 2.0 + 0.03*toRemove`.
   - If `fieldBefore != nil` and `probe.count >= 6`, poll every 250 ms until `capture().before` contains `probe`.
   - If it never lands, sleep until the deadline + 1.5 s.
   - Then restore the clipboard.

**`forget()`:** `typed=""`, restore the clipboard.

**`restoreClipboard`:** only the saved plain string is restored (clear the clipboard, then set the string).

## 7. Paster.swift
Not used in the dictation path; LiveTyper does the pasting.
- `paste(text)`: save the clipboard, set `text`, post ⌘V down/up to the HID tap.
- If there was a previous clipboard value, restore it after 2.5 s.

## 8. Overlay.swift
**Panel:** `NSPanel`, borderless and non-activating, 200×52, level `.statusBar`, clear background, shadow, ignores mouse events.
- Collection behavior: canJoinAllSpaces, fullScreenAuxiliary, stationary.
- Position: on the screen containing the mouse (else the main screen), centered horizontally, `y = visibleFrame.minY + 28`.

**Modes:**
- `listening`: mic icon + 14 bars. Each bar is 3 wide, height `4 + level*20`, animated with easeOut 0.08 s. Levels are reset on show.
- `processing`: 3 dots cycling every 300 ms + "Transcribing".
- `message(s)`: one line; auto-hides after 2.5 s.
- `meetingOffer(app)` / `meetingLive(start)`: 360 wide, clickable. Not part of dictation.

**Behavior:**
- Any mode except `listening` clears the live text.
- `level(v)`: shift the bars left and push `min(1, v*4)`.
- `live(text)`: shows the text truncated at the head, up to 2 lines. The panel grows to 520×74 when the text is non-empty and is re-centered.
- Styling: capsule, `white(0.1)` at 0.94 opacity, 0.12 white stroke, padding 16/10.

**Cue:** `NSSound` "Tink" at start and "Pop" at stop, volume 0.25.

## 9. Pipeline.swift
**Regexes:**
- `signals`: `(?i)\b(sorry|actually|no[,\s]+wait|wait[,\s]+no|oh[,\s]+no|i mean|scratch that|forget that|never ?mind|correction|number (one|two|1|2)|first(ly)?|second(ly)?)\b`
- `corrections`: `(?i)\b(sorry|no[,\s]+wait|wait[,\s]+no|oh[,\s]+no|i mean|scratch that|forget that|never ?mind|correction|actually|let me rephrase)\b`
- `aside`: `,\s*[A-Za-z']+(?:\s+[A-Za-z']+)?\s*,`

**`hasCorrectionSignal(text, learned)`:**
- True if `corrections` or `aside` matches.
- Or if `" "+lower+" "` contains `" "+sig+" "` or `" "+sig+","` for any learned signal.

**`run(raw, tone, config, polisher, examples, context, onPartial)`:**
1. `text = chineseScript(applyDictionary(applyCommands(raw), dict))`. This applies in every tone, verbatim included.
2. If the tone instruction is empty (verbatim) or there is no polisher, return `text`.
3. `pre = stripFillers(text)`.
4. Fast path, when all of these hold:
   - tone is `casual` or `supercasual`
   - `pre` has ≤ 4 space-separated words and no `\n`
   - `signals` doesn't match `pre`
   - `hasCorrectionSignal(text)` is false

   Then return `questionMarks(applyCountedLists(applyLists(layoutMessage(casual(pre, keeping: dictTerms, superCasual: tone=="supercasual")))))`.
5. Otherwise: `polished = polisher.polish(pre, ...)`.
6. If `looksUnfaithful(pre, polished, allowed: dictTerms)`, return `pre` (guardrail).
7. `out = chineseScript(applyDictionary(polished))`. If `context.before` is non-empty, `out = stripEcho(out, before)`.
8. Return `questionMarks(applyCountedLists(applyLists(layoutMessage(layoutGreeting(out)))))`.

## 10. Polisher.swift (local llama-server + prompts)
**`start()`:**
1. Run `pkill -f "llama-server.*--port <port>"`, then sleep 300 ms.
2. The model file must exist.
3. Spawn: `-m <gguf> --host 127.0.0.1 --port <p> -ngl 99 -c 4096 -np 1 --reasoning-budget 0 --reasoning-format none --chat-template-kwargs {"enable_thinking":false} --no-webui --log-disable --spec-type ngram-mod`, with stdout/stderr sent to null.

**`stop()`:** terminate the process.

**Readiness:**
- `isHealthy`: `GET /health`, 1 s timeout, expects 200.
- `waitUntilReady(timeout=120)`: poll every 500 ms.

**`polish()`:**
- `system = systemPrompt(...)`. It must be byte-identical across dictations: Qwen3.5 is hybrid-attention and can only resume from a checkpoint at the end of the system prompt.
- `user = userMessage(...)`.
- If there is no `onPartial` or cloud is active, use `complete(maxTokens 1024)`. Otherwise use `stream`.
- An empty result falls back to `raw`.

**`userMessage`:** `"[tone: T"`, then optionally:
- `" | window: <title prefix 60>"`
- `" | before: \"<before suffix 160>\""`
- `" | after: \"<after prefix 100>\""`

then `"]\n" + raw`. Newlines in these fields become spaces and `"` becomes `'`.

**`systemPrompt` layout (order matters):**
1. Fixed rules.
2. Dictionary block.
3. Memory block.
4. Tones block: `### name\n<instruction>` for sorted non-empty tones. Each tone gets up to 3 corrections for that tone as `"heard[..200]" -> "wanted[..200]"`, under the line "This speaker edited earlier … match their style exactly".
5. Learned-signals sentence.
6. Message-format explanation plus two examples.
- Dictionary line: `Custom vocabulary: Term (sounds like: a, b); … If the transcript has a word or phrase that sounds like one of these, replace it with the term exactly as written here, even in a lowercase tone.`
- Memory line: `About the speaker (background … never insert it into the output): …`

**Key rules in the prompt:**
- "Work in two passes."
- **Pass 1, self-corrections:**
  - Signals: sorry, no wait, actually, I mean, scratch that, correction, or an abrupt restart. Delete the abandoned phrase up to and including the signal.
  - Garbled signals: "I know / no way / no wake / no weight" = no wait; "so we / sore" = sorry; "scratch it" = scratch that. A nonsense mid-sentence aside counts as a misheard "no wait".
  - A short fragment after the signal replaces the part it stands in for, even mid-sentence ("grilled cheese, no wait, dino nuggets" → "I'll do dino nuggets for lunch").
  - A real apology is kept.
  - "scratch that / forget that / never mind" delete the whole preceding sentence, and nothing may be borrowed from it.
- **Pass 2, cleanup:**
  - Remove fillers (um, umm, uh, er, like, you know, I mean), stutters and repeated words. Fix grammar and punctuation.
  - Keep wording, greetings, swearing and laughter (haha, lol, oh, wow, ugh are content). Never add, answer, summarize or comment.
- **Structure:**
  - Keep line breaks.
  - A greeting goes on its own line followed by a blank line.
  - A sign-off goes after a blank line, with the name on the next line ("Kind regards\nTaylor"); keep it lowercase in casual tone.
  - New paragraph on a topic change or after 3–4 sentences.
  - A spoken count becomes a numbered list. Enumerations become numbered lists; "bullet point", "dot point", "next point" or 3+ parallel items become `- ` bullets.
  - Keep emails, URLs, numbers, dates and emoji exactly.
  - Brand spelling: "whisper flow" → Wispr Flow, ChatGPT, iPhone, YouTube.
  - Questions get `?`, including statement-questions.
  - Quote mentioned words, titles and reported speech.
  - Includes one full professional email example.
- **Metadata:** the bracket line is context only and never repeated. If `before` ends mid-sentence, continue it: no capital first letter, no greeting. "Reply with the cleaned transcript only."

**HTTP requests:**
- `stream`: `POST /v1/chat/completions` with `{messages, temperature 0, max_tokens, stream:true, cache_prompt:true, chat_template_kwargs:{enable_thinking:false}}`, header `Accept-Encoding: identity`, 60 s timeout, ephemeral session.
  - Parse SSE `data: ` lines until `[DONE]`. Append `choices[0].delta.content`.
  - `stripThink`: return what follows `</think>`. If only `<think>` has appeared, return `""`. Trim.
  - Log `timings` (`prompt_n`, `prompt_ms`, `predicted_n`, `predicted_ms`).
- `complete(system, user, maxTokens, timeout=60)`:
  - Try `cloud.complete` first if cloud applies; on error, fall back to local.
  - Same body with `stream:false`. Parse `choices[0].message.content`, cut everything before `</think>`, trim.
  - Log `draft_n_accepted/draft_n`.

**Meeting prompts:**
- `notesFormat`:
  - "Write the notes in the language the meeting was held in."
  - Exact markdown: `## Summary` (3–6 bullets), `## Decisions` ("- None" if empty), `## Action items` (who/what/by when; "- None"), `## Your to-dos` (only tasks for "You", verb-first, with deadline; "- None").
  - "Be concrete… Never invent anything."
- `summarizeMeeting`: map-reduce.
  - Split into chunks of 1300 words.
  - Chunk system prompt: "You take notes for the person whose lines are labelled "You"; "Them" is everyone else." Plus the memory (first 400 characters) and "When the call mentions them by name, that is "You"." Then notesFormat. `maxTokens=700`, timeout 180 s.
  - Merge groups of 4 joined by `\n\n---\n\n`: "Merge them into one set of notes, removing repetition and keeping every decision and action item." `maxTokens=900`. Repeat until one note remains.
- `meetingTitle`: "title of 3 to 7 words … like a calendar event name (e.g. "Pricing page launch and reel"). Never start with "Meeting", "Notes" or "Summary". Output only the title, no quotes, no full stop." `maxTokens=24`. Trim `"'. \n` and truncate to 80 characters.

## 11. Cloud.swift (Groq)
- Base URL `https://api.groq.com/openai/v1`, auth header `Bearer <key>`.
- **`init?` routing:** needs `cloudEnabled` and a non-empty trimmed key.
  - `.meetings` requires `cloudMeetings`.
  - `.dictation` requires `cloudDictation=="all"`, or `"long"` with `long==true`. Long means > 15 s or a correction signal in the live text.
- **`transcribe`:** `POST /audio/transcriptions`, multipart with boundary `jezflow-<uuid>`.
  - Fields: `model`, `response_format=verbose_json`, `temperature=0`, optional `language`. No `prompt` is sent, because Whisper echoes prompt terms.
  - File: `audio.wav`, 16-bit PCM, 16 kHz mono, 44-byte header, samples clamped to [-1, 1] × 32767.
  - Timeout 20 s. Requires status 200 and `text`. Returns `filterWhisper(text, segments, seconds)`.
- **`complete`:** tries `[llmModel] + ["openai/gpt-oss-120b", "openai/gpt-oss-20b"]`.
  - Moves to the next model only if the error contains "does not exist", "not have access", "decommissioned", "model_not_found", "too large", "rate_limit", "Rate limit" or "429". Any other error is thrown.
  - `POST /chat/completions`, `{model, messages, temperature 0, max_tokens: min(n, 900)}`.
  - Models starting with `qwen` get `reasoning_effort:"none"`. Models containing `gpt-oss` get `reasoning_effort:"low"` and `reasoning_format:"hidden"`.
  - `User-Agent: JezFlow/0.1`, timeout 30 s. Cut before `</think>` and trim.
- Live text always stays local.

## 12. Transcriber.swift (FluidAudio Parakeet TDT, CoreML)
- **`load(v2|v3)`:** `AsrModels.downloadAndLoad` then `AsrManager.loadModels`.
- **Live freeze constants:** `freezeAfter = 8*16000`, `freezeSearch = 80000...120000` (5 s to 7.5 s).
- **`liveUpdate(samples)`:**
  1. If `samples.count - frozenSamples > 128000`, `cut = frozenSamples + quietestPoint(...)`.
  2. `frozenText.append(transcribe(samples[frozen..<cut]))`, a full decode with vocabulary rescoring. Set `frozenSamples = cut`.
  3. Decode the tail `samples[frozen...]` with a fresh `TdtDecoderState`, without rescoring.
  4. Return `(frozenText + [tail]).nonEmpty.joined(" ")`.
- **`liveFinish(samples)`:** `transcribe(tail)` (rescored), join with `frozenText`, then `beginLive()` to reset.
- **`quietestPoint`:** a 3200-sample (200 ms) window stepping 800 samples through the range. Returns the start with the minimum Σx².
- **`setVocabulary(terms, extra)`:**
  1. `names = dictionaryTerms(terms)`, plus `extra` entries not already present (case-insensitive).
  2. Trim and keep names with ≥ 4 characters. If unchanged, do nothing.
  3. Load `CtcModels` (parakeet-ctc-110m) once.
  4. `CustomVocabularyContext(terms, minSimilarity: 0.75, minTermLength: 4)`, then `VocabularyBoostingSession(config: spotterRescueEnabled:false)`.
- **`transcribe`:** `manager.transcribe(samples, decoderState:)`. If boosting is on and token timings exist, call `boosting.rescore(text, tokenTimings, audioSamples)` and use the result if `wasModified`. Trim.
- **Porting note:** these are Swift/CoreML-only APIs. A Rust port needs its own equivalents: a Parakeet runtime such as ONNX or sherpa-onnx, and CTC keyword spotting.

## 13. TextRules.swift: all functions
**Commands, emoji and numbers**
- **`applyCommands(t)`:** `applyEmoji` → `applyNumbers`, then these regexes in order:
  - `[,.]?\s*\b(new paragraph|paragraph break)\b[,.]?\s*` → `\n\n`
  - `…(new line|newline|line break)…` → `\n`
  - `…(bullet point|dot point|new bullet|next bullet)…` → `\n- `
  - `\s*\b(full stop|fullstop)\b[,.]?` → `.`
  - `\s*\bquestion mark\b[,.]?` → `?`
  - `\s*\bexclamation (mark|point)\b[,.]?` → `!`
  - `\b([\w.-]+) at ([\w-]+)((?: dot [a-z]{2,4})+)\b` → `$1@$2$3`
  - `\b([\w-]+)((?: dot [a-z]{2,4})+)\b` → `$1$2`
  - then ` dot ([a-z]{2,4})\b` → `.$1`
  - finally `" \n"` → `"\n"` and trim.
  - Tests: "first line new line second line, new paragraph third" → `first line\nsecond line\n\nthird`; bullets → `shopping list\n- milk\n- eggs\n- bread`; "sam at example dot com" → `sam@example.com`; "acmehub dot com dot au" → `acmehub.com.au`; "question mark" → `?`; "full stop" → `.`
- **`applyEmoji`:**
  - About 100 names sorted longest-first. Spaces inside a name match `[\s,\-]+`.
  - Regex: `(?i)\s*[,.]?\s*\b(?:(?:a|an|the)\s+)?(NAMES)[\s,\-]+emojis?\b[,.]?`
  - Replace with the glyph, preceded by a space unless the text before is empty or ends in whitespace. Loop until no match.
  - Tests: "so funny laugh cry emoji, anyway" → `so funny 😂 anyway`; "a thumbs up emoji" → `👍`; "fire emoji fire emoji" → `🔥 🔥`; "thumbs-up emoji" → `👍`; "the emoji keyboard" is unchanged.
- **`applyNumbers`:** tokenizes on spaces.
  - `parseNumber` handles digits (with a following big scale), small words 0–20 and the tens, `a/an` before a scale, `hundred` (multiplies), thousand/million/billion/trillion (add to total), `and` between number words, and `point` + a digit word < 10 (decimal).
  - Units: dollar/dollars/bucks → `$` prefix; percent or "per cent" → `%`; `k`.
  - Converted only if the value ≥ 10, or it has a decimal, a unit or a scale. Digits already written are converted only with a unit or scale.
  - Big scales are written as `N scale` (formatted with `%g`). Numbers ≥ 10000 get comma grouping. Trailing punctuation is kept.
  - Tests: "two billion dollars" → `$2 billion`; "one point five billion" → `1.5 billion`; "twenty dollars" → `$20`; "fifteen percent" → `15%`; "two things" unchanged; "twenty five" → `25`; "a hundred and fifty dollars" → `$150`; "twenty thousand" → `20,000`; "number one" unchanged; "two thousand and twenty six" → `2026`; "one point five million bucks" → `$1.5 million`.

**Cleanup before the model (`stripFillers`)**
- **`stripFillers(t)`:** `applySwaps(applyRepeats(applyRestarts(applyScratch(t))))`, then:
  1. articleRestart
  2. filler regex `(?i)(^|[\s,])(um+|uh+|uhm|erm?|hmm+|mm+)\b[,.]?\s*` → `$1`
  3. stutter `(?i)\b(\w+)([,.]?\s+)\1\b(?=[\s,.!?]|$)` → `$1`, applied twice
  4. collapse `\s{2,}`, strip leading `^[,\s]+`, trim
  - Tests: "Um, so, tomorrow…" → `so, tomorrow is fine. See you then.`; "we we we ship … tomorrow, tomorrow we look" → `we ship on tuesday tomorrow we look`; "Hmm. The umbrella" → `The umbrella is there.`
- **`applyScratch`:** signal `(?i)\b(?:actually\s+)?(?:scratch that|forget that|never ?mind)\b[,.!?]?\s*`.
  - Walk back over whitespace. If at a sentence start (text start or a terminator), step over the terminator into the previous sentence.
  - Delete back to the previous terminator (`.!?`), keeping the text after the signal.
  - Tests: "…tomorrow is fine, actually scratch that, let's do Thursday instead." → `let's do Thursday instead.`; "…Monday. Then um Tuesday scratch that Wednesday works" → `We agreed on Monday. Wednesday works`; "She's a horrible person. Never mind. She's…" drops the previous sentence; "nevermind" works as one word.
- **`applySwaps`:** `(?i)\b([\w']+)[,.]?\s+(?:actually|no|sorry|i mean)[,.]?\s+([\w']+)\b`.
  - If x ≠ y and both are in the same class (pronouns, weekdays, months, one–twelve, times of day) or both are integers, replace the match with y.
  - Tests: "she actually he" → `he`; "Tuesday, actually Thursday," → `Thursday,`; "5 no 6 chairs" → `6 chairs`; "it's actually good" unchanged.
- **`applyRestarts`:** signal `(?i)[,.]?\s*\b(?:sorry|no[,\s]+wait|wait[,\s]+no|i mean|correction|let me rephrase)\b[,.]?\s*`.
  - `before` = the last 12 words of the current sentence. `after` = the next 6 words.
  - Find the longest run where `before[k...]` matches the start of `after` (normalized: lowercase, punctuation trimmed).
  - Accept if the run length ≥ 2, or it is 1 and `before[k]` is in `starters` (because, so, and, but, if, when, then, can, could, would, should, i, we, you, he, she, they, it, the, to, let's, please, send, tell).
  - On accept, delete from `before[k]` through the signal.
  - Tests: "very quiet sorry today's weather…" → `today's weather is very sunny`; "John no wait send it to Alex" → `send it to Alex by Friday`; "I want to go, sorry, I need to leave" → `I need to leave`; "sorry I'm late…" kept.
- **`applyRepeats`:** needs ≥ 6 words.
  - For each i and gap 0–4, with `j = i+3+gap`: if words i..i+2 equal words j..j+2 and `word[j-1]` is not a conjunction (and, or, but, then, so, because, if, when), delete `words[i]..<words[j]` and recurse.
  - Tests: "i have a thesing what thief i have a few things" → `…i have a few things…`; "we ship on thursday and we ship on friday" kept.
- **articleRestart:** `(?i)(^|(?<!\b(?:the|a|an|of|in|on|at|by|to|for)\s)\b)(\w+)[,]?\s+(a|an|the)\s+\2\b` → `$1$3 $2`.
  - "fair a fair bit" → `a fair bit`. "the time the time" kept.
  - Porting note: this needs a variable-length lookbehind. Rust's `regex` crate lacks it; use `fancy-regex` or rewrite the rule by hand.

**Casual fast path**
- **`casual(t, keeping, superCasual)`:**
  1. Lowercase the first letter after start or `.?!`.
  2. `\bI\b` → `i`. Remove `[!;:]+`. `\b(haha+|lol|lmao)[.,]` → `$1`.
  3. Super casual: remove all commas.
  4. Collapse spaces and trim.
  5. Super casual: strip trailing `.` and `,`.
  6. Restore dictionary terms with ≥ 3 characters (lowercase form → term, plain substring replace).
  - Tests: super casual "Sounds good, see you at eight. Bring the charger." → `sounds good see you at eight. bring the charger`; casual keeps the final stop; "Haha. No, I…" → `haha no, i really don't…`; names kept: `see you at Acme Hub, Alex i'll bring Instagram stuff.`

**Layout after the model**
- **`layoutMessage`:**
  - Only for single-line text.
  - Greeting: `(?i)^((?:dear|hi|hey|hello|good morning|good afternoon|good evening|morning|afternoon|evening)\b(?:[^,.!\n]{0,40}[,.!]|\s+[A-Za-z][\w'-]*(?=\s)))\s*`
  - Sign-off: `(?i)\s+((?:kind regards|best regards|warm regards|warmest regards|regards|cheers|many thanks|thanks|thank you|best|all the best|talk soon|take care|speak soon)[,.]?)(?:\s+([A-Za-z][\w'-]*(?:\s+[A-Za-z][\w'-]*)?))?[.]?\s*$`
  - The body must have ≥ 2 words. Output: `greeting\n\nbody\n\nsignoff[\nname]`.
  - Tests: "dear robin no worries … kind regards jess" → `dear robin\n\nno worries…weekend\n\nkind regards\njess`; "Hi Alex, thanks for the deck. Cheers" → `Hi Alex,\n\nthanks for the deck.\n\nCheers`; "no worries, cheers" and "hey, thanks for that" unchanged.
- **`layoutGreeting`:** only if the text contains `\n\n`.
  - Greeting regex like above but ending `\s+(?=\S)`.
  - Output: `head\n\n` + rest. The rest's first letter is uppercased only if the greeting starts uppercase.
  - Test: "Hi Jordan, thanks…" → `Hi Jordan,\n\nThanks…`. Chat text without `\n\n` is untouched.
- **`stripEcho(out, before)`:** `tail` = the last 12 words of `before`; needs ≥ 4.
  - For n from `min(|tail|, |out|)` down to 4, and j in 0...2: if the last n normalized words of `tail` equal `out[j..<j+n]`, drop the first j+n words of the output.
  - Test: echo removed → `as seen above we are missing paragraphs`.
- **`applyLists`:**
  - Marker regex: `(?i)(^|[.,;:!?]\s*|\s)(?:(?:firstly|secondly|thirdly|fourthly|fifthly|first of all|second of all|number (?:one|two|three|four|five|six))|(?<=[.,;:!?]\s)(?:first|second|third|fourth|fifth)|^(?:first|second|third))\b[,:]?\s*`
  - Needs ≥ 2 matches. Replace each (in reverse) with the trimmed lead punctuation + `\n- `.
  - Then `[,;]\n- ` → `.\n- `, leading `^\n- ` → `- `, trim.
  - Tests: firstly/secondly/thirdly → bullets; "Three things: number one …, number two …" → `Three things:\n- the pricing slide.\n- the roadmap.`; a single marker is unchanged; "the first time" is unchanged.
- **`applyCountedLists`:**
  - Skipped if the text already has `\n1.` or `\n- `.
  - Lead: `(?i)\b(two|three|four|five|six|2|3|4|5|6)\s+(things|reasons|points|steps|options|ways|issues|updates|questions|items|ideas|problems|tips|takeaways|decisions|requests|asks)\b([^:.\n]{0,80})[:.]\s*`
  - The body runs up to `[.!?](?=\s|$)|\n`.
  - Split items on `;`, `, and`, `, or` and `,`. If there is one item too few, split the last item on ` and ` / ` or `. The count must equal n.
  - A lead ending in `.` becomes `:`. Output: `lead\n1. …\n2. …` + `\n` + tail.
  - Tests: `…really well:\n1. it's insanely fast\n2. it's insanely accurate\n3. it gets names right`; "Two reasons: …, and … Let me know." → `…\n2. the timing works\nLet me know.`; a count mismatch is unchanged.
- **`questionMarks`:** processed per line, per sentence split on `.?!`.
  - Strip a list prefix `^(?:- |\d+[.)] )` before testing.
  - `questionStart`: `(?i)^(?:(?:are|can|could|do|does|did|is|will|would|should|shall|have|has|were|was|am)\s+(?:you|we|i|it|there|they|he|she|this|that|anyone|someone)\b|(?:what|when|where|who|why|how)['’](?:s|re|d|ll)\s+\w+|(?:what|when|where|who|why|how)\s+(?:is|are|do|does|did|can|could|will|would|should|have|has|was|were|about|time|many|much|long)\b)`
  - If it matches and the terminator is none or `.`, use `?`.
  - Tests: adds `?` at the end; "Can you send it by Friday. Thanks." → `…Friday? Thanks.`; "What we need is time." kept; list items get `?`.

**Dictionary and learning**
- **`parseDictionary(lines)`:** each line is `Term`, `Term: a, b`, `Term = a` or `Term (a, b)`. Aliases split on `,` or `;`.
- **`dictionaryTerms`:** the terms only.
- **`applyDictionary(t, lines)`:** for each entry, variants (term + aliases) sorted longest-first, each with ≥ 2 characters.
  - Pattern: `(?i)\b<escaped, with spaces → [\s-]*>\b` → term.
  - Tests: zentrow → Zentro; "whisper flow" → Wispr Flow; "the molisha" → Demolisher; "jezflow" → JezFlow; "zentrowle" unchanged.
- **`contextNames(text, limit 15)`:**
  - Split words and trim punctuation except `'’-`.
  - A name has ≥ 3 characters, starts uppercase, contains a letter after the first character, is not in `commonCapitalised` (about 150 words), and doesn't also appear lowercase in the text.
  - Group up to 3 consecutive names into one phrase and dedupe case-insensitively.
  - Tests: "Re: Zentro launch — Mail Hi Alex, about the Wispr Flow reel… Acme Hub" → `[Zentro, Alex, Wispr Flow, Acme Hub]`; "Thanks Monday Hi The Window" → `[]`.
- **`learnNames(heard, corrected)`:** LCS diff over normalized words (both sides < 400 words).
  - A differing span is extended over matching words that are capitalized in the correction (span < 3).
  - Emit `(term, alias)` if the heard span has ≤ 4 words, the fixed span has ≤ 3 words and is all capitalized non-common words, and term ≠ alias with ≥ 3 characters.
  - Tests: `Zentro<-zentrow`, `Wispr Flow<-whisper flow`, `Demolisher<-the molisha`; plain edits give nothing.
- **`learnSignals(heard, corrected)`:** heard needs ≥ 4 words.
  - Find deleted runs of ≥ 3 words that are followed by kept text. Emit the 2-word tail, or the 1-word tail if it is not a common word.
  - Test: "…Vegemite toast i know kfc…" → `["i know"]`.
- **`mergeDictionary(lines, learned)`:** add the alias to an existing term's line, or append `Term: alias`.

**Guardrail, Whisper filter and misc**
- **`looksUnfaithful(input, output, allowed)`:** words are lowercase alphanumeric plus `'`.
  - If the input has < 6 words, return false.
  - True if `out/in < 0.35` or `> 1.6`.
  - True if novel output words longer than 3 characters (not in the input or `allowed`) make up more than 30% of the output.
- **`filterWhisper(text, segments, seconds)`:**
  - Per segment, drop it if:
    - `no_speech_prob > 0.6` and `avg_logprob < -0.7`
    - or `avg_logprob < -1.2`
    - or `compression_ratio > 2.4`
    - or it is a ghost phrase: thank you, thanks for watching, thank you for watching, bye, you, the end, "subtitles by the amara.org community", please subscribe, like and subscribe, thank you very much, i'm sorry, so, oh, um
  - Return `""` if `seconds > 1` and the rate is > 6 words/s.
  - Tests: "Thank you." over 3 s → `""`; keeps only "See you Thursday."; 14 words in 2 s → `""`.
- **`chineseScript(t, traditional)`:** if the text contains U+4E00–9FFF, apply the ICU transform `Hans-Hant`. In Rust, use OpenCC or similar.
- **`containsChinese`**, **`looksEnglish`** (stopword ratio ≥ 0.15, or ≤ 3 words with ≥ 1 hit, and no Chinese), **`meetingLine`**, **`capitaliseProperNames`**: meeting-only.
- **`wordErrorRate`:** Levenshtein distance over words divided by the reference length. Eval only.

## 14. Porting notes
- **Dead or quirky code:** `Paster` is unused. The two `typer.finish` branches in `finishRecording` are identical. The peak check uses the signed `max`. The hotkey tap swallows the `;` keyUp even when idle.
- **Streaming:** `onPartial` is never passed during dictation, so only the non-streaming `complete` path runs in practice.
- **Timings to keep:**

| Setting | Value |
|---|---|
| Tap/hold threshold | 0.5 s |
| Live update cadence | 400 ms (min 120 ms) |
| Live update starts after | 0.5 s of audio |
| Minimum recording | 0.25 s |
| Silence threshold | peak 1e-5 (1e-6 after an empty transcript) |
| Long dictation | > 15 s |
| Mic kept warm | 20 s |
| Live-paste landing poll | 6 × 100 ms |
| Clipboard restore | 2 s + 30 ms per backspace (+1.5 s if unconfirmed) |
| Error auto-clear | 5 s (silence) / 4 s (other) |
| Overlay message | 2.5 s |
| Tap retry | 3 s |
| llama-server context | 4096, port 8765 (notes model 8767) |