> Reference spec of the legacy Swift app (now in `dep/`, git-ignored; absolute path `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`). Where this spec and the Swift source disagree, the Swift source wins. Prompts, tone texts and regexes must be copied verbatim from the Swift source.

# JezFlow Meetings Subsystem: Porting Spec

All files are in `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`. The main ones are `MeetingSession.swift`, `MeetingAudio.swift`, `Meeting.swift`, `WhisperServer.swift`, `ModelCatalog.swift`, `NameSources.swift`, `Transcriber.swift` and `AppDelegate.swift`. Supporting code is in `Store.swift`, `Config.swift`, `Cloud.swift`, `TextRules.swift`, `Polisher.swift`, `Overlay.swift` and `AudioRecorder.swift`.

## 1. Paths and config keys

- Base directory: `~/Library/Application Support/JezFlow/`
  - `config.json`
  - `models/` (GGUF files and the whisper model)
  - `meetings/<UUID>.json`, one file per meeting
- Meeting-related keys in `config.json`. Every key is decoded with `decodeIfPresent`, so a missing key falls back to its default.

| key | default | meaning |
|---|---|---|
| `meetingDetect` | `true` | Offer to take notes when a call is detected |
| `meetingEngine` | `"auto"` | `"auto"` = Parakeet plus SenseVoice/Whisper fallback for other languages. `"english"` = Parakeet only |
| `meetingFarEngine` | `"whisper"` | `"whisper"` or `"parakeet"` for the "Them" side |
| `whisperServerPath` | `/opt/homebrew/bin/whisper-server` | whisper.cpp binary |
| `whisperModel` | `ggml-large-v3-turbo.bin` | file name inside `models/` |
| `notesModel` | `Qwen3.5-9B-Q4_K_M.gguf` | local LLM for notes. `""` = use the polish model |
| `chineseScript` | `"traditional"` | `"traditional"` or `"simplified"` |
| `useContacts` | `false` | Enables NameSources |
| `microphone` | `""` | preferred input device UID (`"builtin"` is a special value) |
| `asrVersion` | `"v2"` | Parakeet `v2` or `v3` |
| `llamaPort` | `8765` | polish llama-server port. The notes model uses **8767** |
| `cloudEnabled` / `cloudAPIKey` / `cloudMeetings` (`true`) / `cloudASRModel` (`whisper-large-v3-turbo`) / `cloudLLMModel` (`qwen/qwen3.8-27b`, can be overridden by env `JEZ_CLOUD_LLM`) | | Groq settings |

- The cloud is used for meetings only when all three hold: `cloudEnabled`, a non-blank trimmed `cloudAPIKey`, and `cloudMeetings`.
- The Swift logger subsystem is `com.jez.flow`, with categories `meeting`, `tap`, `whisper`, `asr`, `cloud` and `app`.

## 2. Data model (`Meeting.swift`) and JSON storage

**`MeetingSegment`**
- `id`: UUID
- `time`: Double, seconds from meeting start
- `speaker`: `"You"` (mic) or `"Them"` (system audio)
- `text`: String

**`MeetingTodo`**
- `id`: UUID
- `text`: String
- `done`: Bool, default false

**`Meeting`**
- `id`: UUID
- `title`: String
- `titleAuto`: Bool, default true. It becomes false once the user edits the title.
- `todos`: [MeetingTodo]
- `app`: String?
- `start`: Date
- `end`: Date?. `nil` means the meeting is live.
- `segments`: [MeetingSegment]
- `summary`: String, default `""`
- `notes`: String, default `""` (the user's own notes)

**JSON encoding**
- The encoder is Swift's default `JSONEncoder` with `[.prettyPrinted, .sortedKeys]`.
- **Dates use the default strategy: a Double of seconds since 2001-01-01 (Apple reference date), not Unix time.** Add 978307200 to convert to Unix seconds.
- UUIDs are uppercase strings.
- Decoding requires `id`, `title` and `start`. Every other field is optional and gets its default. `MeetingSegment` and `MeetingTodo` use synthesized Codable, so all their fields are required.

**Derived values**
- `isLive`: `end == nil`
- `duration`: `(end ?? now) - start`
- `words`: sum over segments of the count of words split on a single space

**Transcript**
- Sort segments by `time`, then output one line per segment: `"[{clock}] {speaker}: {text}"`, joined with `\n`.
- `clock(t)`: `s = Int(t)` (truncate). If `s >= 3600` the format is `H:MM:SS`, otherwise `M:SS` (for example `0:05`, `12:34`, `1:02:03`).

**Markdown export**
```
# {title}

{start: medium date, short time} · {Int(duration/60)} min[ · {appDisplayName(app)}]

{summary}                      (only if non-empty)

## To-dos

- [x] text / - [ ] text        (only if todos exist)

## My notes

{notes}                        (only if non-empty)

## Transcript

{transcript}
```
The export ends with a trailing newline.

**Parsing to-dos (`refreshTodos(from: summary)`)**
1. Find the first `"## Your to-dos"`. If it is missing, change nothing.
2. Take the lines after it until one starts with `"## "`.
3. Trim each line and keep only those starting with `-`.
4. Drop the leading `-` and trim again.
5. Discard empty items and items equal to `none` (case-insensitive).
6. Rebuild the todos, keeping `done` from any existing todo with an exactly equal text.

**Store operations (`Store.swift`)**
- `addMeeting`: insert at index 0, then save.
- `update(id, closure)`: mutate, then save the whole file. Every segment append or extension rewrites the JSON.
- `appendSegment`
- `finishMeeting`: set `end = now` if it is nil, and clear `liveMeetingID`.
- `deleteMeeting`: remove the file and the entry.
- `loadAll`: read every `*.json` in `meetings/`, silently skip files that fail to decode, and sort by `start` descending.
- State: `summarizing: Set<UUID>` and `liveMeetingID`.

## 3. Call detection (`MeetingDetector`)

**Polling:** a `Timer` fires every **3 s** and calls `tick()`.

**`tick()`**
1. `micBusy` = `SystemAudioTap.microphoneInUse()`. This reads the default input device and checks `kAudioDevicePropertyDeviceIsRunningSomewhere != 0`.
2. If `isRecording()` is true (a JezFlow meeting is running, or `recorder.isRunning`), reset `idleSince` and `busyTicks` and return. JezFlow's own capture must not count as a call.
3. If the mic is not busy: set `busyTicks = 0`. If `announced`, start or continue the `idleSince` clock. After **more than 20 s** idle, set `announced = false`, reset `idleSince`, and call `onEnded()`. Return.
4. If the mic is busy: set `idleSince = nil`, increment `busyTicks`, and set `call = currentCall()`.
5. If `call == nil` and `busyTicks >= 5` (about 15 s), use the frontmost app's `localizedName` as the call, unless that name is `"JezFlow"`.
6. If there is a call and `!announced`: set `announced = true` and call `onDetected(call)`.

**`markHandled()`** sets `announced = true`. It runs when an offer is dismissed, times out, or a meeting stops, so the same call is not offered again.

**`currentCall()`**
1. Check every running app. If its bundle id is in `apps` and the app is not terminated, set `currentCallBundle = id` and return the display name.
2. Otherwise check each running browser. Read all window titles through the Accessibility API (`AXUIElementCreateApplication(pid)`, then `kAXWindowsAttribute`, then `kAXTitleAttribute` for each window). If a lowercased title contains any `titleHints` entry, set `currentCallBundle` to the browser's bundle id and return the browser's `localizedName` (or `"browser"`).
3. Otherwise set `currentCallBundle` to the frontmost app's bundle id and return nil.

Note that the check in step 1 only asks whether the app is running, not whether it holds the mic. The mic-busy check in `tick()` is what ties detection to a call.

**Call apps (bundle id → name)**
- `us.zoom.xos` Zoom
- `com.microsoft.teams2`, `com.microsoft.teams` Teams
- `com.apple.FaceTime` FaceTime
- `com.tinyspeck.slackmacgap` Slack
- `com.hnc.Discord` Discord
- `com.skype.skype` Skype
- `com.cisco.webexmeetingsapp`, `com.webex.meetingmanager` Webex
- `com.loom.desktop` Loom
- `com.ringcentral.glip` RingCentral
- `com.logmein.GoToMeeting`, `com.gotomeeting.GoToMeeting` GoTo
- `com.around.Around` Around
- `com.electron.lark` Lark
- `com.tencent.meeting` Tencent Meeting
- `com.zhumu.zhumu` Zhumu
- `com.bluejeans.app` BlueJeans
- `com.dialpad.desktop` Dialpad
- `com.riverside.desktop` Riverside
- `com.jitsi.jitsi-meet` Jitsi
- `com.whereby.desktop` Whereby
- `com.streamyard.desktop` StreamYard
- `com.pop.desktop` Pop
- `com.readdle.spark` Spark
- `com.google.meet` Google Meet
- `com.linphone.linphone` Linphone
- `com.facebook.archon`, `com.facebook.archon.developerID` Messenger
- `com.apple.MobileSMS` Messages
- `net.whatsapp.WhatsApp` WhatsApp
- `com.viber.osx` Viber
- `org.telegram.desktop`, `ru.keepcoder.Telegram` Telegram
- `com.signal.desktop`, `org.whispersystems.signal-desktop` Signal
- `com.tencent.xinWeChat` WeChat
- `jp.naver.line.mac` LINE

**Browsers:** `com.google.Chrome`, `com.apple.Safari`, `company.thebrowser.Browser`, `org.mozilla.firefox`, `com.microsoft.edgemac`, `com.brave.Browser`, `com.vivaldi.Vivaldi`, `com.operasoftware.Opera`, `com.google.Chrome.canary`, `org.chromium.Chromium`, `com.apple.Safari.WebApp`.

**Title hints** (substring match on the lowercased title): `messenger`, `facebook.com/call`, `meet.google`, `google meet`, `meet -`, `zoom`, `teams`, `webex`, `whereby`, `around.co`, `huddle`, `jitsi`, `discord`, `goto`, `dialpad`, `riverside`, `streamyard`, `zoho meeting`, `bluejeans`, `skype`, `call -`, `video call`, `meeting`.

## 4. AppDelegate orchestration

**Wiring**
- `detector.isRecording = { meeting != nil || recorder.isRunning }`
- `onDetected(app)`: ignored if `!config.meetingDetect` or a meeting is already running. Otherwise set `meetingOffer = app`, render the overlay, and after **45 s**, if the offer is still for the same app, clear it and call `markHandled()`.
- `onEnded`: clear the offer.
- Overlay actions:
  - start → `startMeeting(app: meetingOffer)`
  - dismiss → clear the offer and `markHandled()`
  - stop → `stopMeeting()`
- Menu item text is "Start Meeting Notes" or "Stop Meeting Notes". Toggling it starts with `app: detector.currentCall()`.

**Overlay**
- `meetingOffer(app)`: pill 360×52, text "Meeting in {app}", buttons "Start taking notes" and ×.
- `meetingLive(start)`: red dot, "Taking notes · {clock}" updating every 1 s, and a "Stop" button.
- Precedence when rendering: live first, then offer.

**`startMeeting(app)`**
1. Ignore the call if a meeting is already running. Clear the offer.
2. Title: `"{app} call"`, or `"Meeting"` if there is no app, then `" · "`, then the date formatted as `"EEE d MMM, h:mm a"`.
3. Call `store.addMeeting` and set `liveMeetingID`.
4. If `meetingFarEngine == "whisper"` and the cloud is off for meetings: if `WhisperServer.available`, start it in the background (fire-and-forget; errors are only logged). Otherwise log it and fall back to Parakeet.
5. Create `MeetingEngine(parakeet: sharedTranscriber, config, whisper)` and `MeetingSession`.
6. Set `session.microphone = config.microphone`. If `app != nil`, call `currentCall()` again and set `session.tapApp = detector.currentCallBundle`.
7. Call `session.start()`. On success:
   - Record `meetingStart`.
   - Start a notes-refresh loop: every **180 s** while the meeting is live, if `words - lastWords >= 40`, run `summarize(id)`.
   - If `!session.systemAudioWorking`, show the message "Only your mic is captured: allow System Audio Recording".
8. On failure: `finishMeeting`, then `deleteMeeting`, then show "Meeting notes failed: {err}".

**`stopMeeting()`**
1. Clear the state.
2. `whisperServer.stop()`.
3. `detector.markHandled()`.
4. `session.stop()`.
5. Poll every **300 ms** until the meeting's `end != nil` (the drain loop sets it), then `summarize(id)`.

**`summarize(id)`**
1. Requires segments to be non-empty, the polisher to be ready, and the meeting not already in `summarizing`.
2. Choose the writer:
   - Default: `Polisher(config, scope: .meetings)`, which goes to the cloud when enabled.
   - If the cloud is off, `notesModel` is non-empty and differs from `config.model`, and the file exists in `models/`: start a separate transient llama-server with `model = notesModel` and `llamaPort = 8767`. Wait up to `waitUntilReady(timeout: 180)`, and stop it afterwards.
3. Run `summarizeMeeting(transcript)`, then apply `chineseScript`. Store the result as `summary` and call `refreshTodos`.
4. If `titleAuto` is true, call `meetingTitle(summary)` and apply `chineseScript` to the result.

**On launch, once the polisher is ready:** summarize every meeting that is finished, has an empty summary, and has segments. Go oldest first (`meetings.reversed()`), one at a time, polling every 1 s.

**On quit:** `meeting?.stop()`.

**Notes prompts (`Polisher.swift`)**
- `summarizeMeeting`: map-reduce.
  - Split the transcript into chunks of **1300** space-separated words.
  - Per chunk, the system prompt is "You take notes for the person whose lines are labelled "You"; "Them" is everyone else…" plus optional memory (first 400 characters) plus `notesFormat`. Limits: maxTokens 700, timeout 180.
  - Merge notes in groups of **4**, joined with `\n\n---\n\n`. Limits: maxTokens 900, timeout 180. Repeat until one set of notes remains.
- `notesFormat` sections: `## Summary` (3–6 bullets), `## Decisions`, `## Action items`, `## Your to-dos`. An empty section is written as `- None`.
- `meetingTitle`: 3–7 words, never starting with Meeting, Notes or Summary. maxTokens 24, timeout 60. Trim `"'. \n` from the result and cap it at 80 characters.

## 5. MeetingSession (live pipeline)

**Components:** `AudioRecorder` (mic), `SystemAudioTap`, and two `Segmenter`s. The mic segmenter keeps the defaults. The tap segmenter is set to `speechFloor = 0.010` and `minSpeechFrames = 15` (300 ms).

**`start()`**
1. Wire `mic.onChunkRaw` to `micSegmenter.push` (16 kHz mono chunks) and `tap.onChunk` to `tapSegmenter.push`.
2. The segmenters' `onUtterance` callbacks enqueue `(samples, start, "You"/"Them")` on the main actor.
3. Set `mic.preferredDevice` and `tap.appBundleID`.
4. Call `mic.start()`. This throws on failure.
5. Start the watchdog.
6. Call `tap.start()`. If it fails, set `systemAudioWorking = false`; this is not fatal.
7. Spawn the drain worker.

**`stop()`** (idempotent)
1. Cancel the watchdog.
2. Stop and shut down the mic.
3. Call `tap.stop()` on a background queue, because destroying the aggregate device can block.
4. Call `finish()` on both segmenters, which flushes any pending speech.

**Mic watchdog** (every 1 s)
1. Take the peak absolute value of the last 16000 samples from `mic.snapshot()`.
2. If the peak is above `1e-5`, the mic is live; reset the timer.
3. After **4 s** of digital silence:
   - Add the current device UID to a `tried` set.
   - Pick the first input device that is not tried, not Continuity, and not built-in while the lid is closed.
   - Set it as `preferredDevice`, then stop and restart the mic.

**Drain worker** (serial, so the two channels never compete for the model)
1. Call `engine.load()` once, ignoring errors.
2. Loop. When the queue is empty: exit if stopped, otherwise sleep 200 ms.
3. Pop the next item (FIFO) and run `engine.transcribe`. Skip it on error or empty text.
4. Clean up the text:
   - English: `TextRules.meetingLine(text, dictionary)`.
   - Otherwise: `chineseScript(applyDictionary(text, dict), traditional)`.
   - Skip the item if the result is empty.
5. Set `end = start + count/16000`.
6. **Merge rule:** if the speaker is the same as the last one, `start - lastEnd < 1.5 s`, and `prev.text.count + text.count < 500`, then set the previous segment's text to `prev + " " + text`. Otherwise append a new segment and remember its id.
7. Update `lastSpeaker` and `lastEnd`. The initial `lastEnd` is -10.
8. After the loop exits, call `store.finishMeeting(id)`.

**`meetingLine` cleanup**
1. Strip fillers. If nothing is left, or there is no letter or digit, return `""`.
2. Apply number conversion, then the dictionary.
3. Replace a standalone `\bi\b` with `I`.
4. Capitalise the proper-name word list, with brand cases ChatGPT, LinkedIn, YouTube and TikTok.
5. Uppercase the first character.
6. Add `.` if the text doesn't end with `.?!`.
7. Apply `questionMarks`.

## 6. MeetingEngine (speech routing)

**`load()`**
- Does nothing if the cloud is on, whisper is present, or `meetingEngine == "english"`.
- Otherwise: `sense = try await SenseVoiceManager.load()`.

**`transcribe(samples, speaker)`** returns `(text, english)`.
1. Compute RMS. If `speaker == "Them"` and `rms < 0.004`, return `("", true)`.
2. Build `audio = normalised(samples)` plus **4800 zero samples** (300 ms tail).
   - Normalise: `peak = max|s|`. Apply gain only if `0.02 < peak < 0.25`, with gain `g = min(4, 0.5/peak)`.
3. If the cloud is on: `cloud.transcribe(audio, prompt: dictionaryTerms)`, which returns `(t, !isNonLatin(t))`. On error, fall through.
4. If `speaker == "Them"` and whisper is present: `whisper.transcribe(audio)`. English is `lang.hasPrefix("en")`; if the language is empty, it is `!isNonLatin(text)`. On error, fall through.
5. `english = parakeet.transcribe(audio)`. If `meetingEngine == "english"`, return `(english, true)`.
6. If the duration (from the un-padded samples) is under **1.5 s**, or `TextRules.looksEnglish(english)`, return the Parakeet text as English.
7. If whisper is present: English per whisper → Parakeet text. Otherwise → `(whisperText, false)`.
8. If SenseVoice is loaded: `sense.transcribe(audio: audio)`, trimmed. Return `(other, false)` if `isNonLatin(other)`, otherwise the Parakeet text.

**Helper checks**
- `isNonLatin`: scalars with value above `0x2E7F` number at least 3, and at least half of all alphabetic scalars.
- `looksEnglish`: split on non-letters (apostrophes kept). The text must contain no CJK (U+4E00–9FFF). It passes if stopword hits / words ≥ 0.15, or if there are ≤ 3 words with at least 1 hit.

## 7. MeetingAudio.swift

**Target format** (`AudioRecorder.targetFormat`): Float32, 16000 Hz, 1 channel, non-interleaved.

**Resampler**
- `AVAudioConverter` from the source format to the target.
- Output capacity: `frameLength * ratio + 16`.
- The input block supplies the buffer once, then returns `.noDataNow`.
- Returns channel 0 as `[Float]`, or `[]` on error.

**SystemAudioTap** (Core Audio process tap, macOS 14.2+; triggers the "System Audio Recording" permission prompt)
1. Choose the processes to tap:
   - Read `kAudioHardwarePropertyProcessObjectList` from the system object.
   - For each process, read `kAudioProcessPropertyBundleID`.
   - It matches if `bid == prefix`, or `bid` starts with `prefix + "."`, or `bid` starts with `root`, where `root` is the first 3 dot-components of the prefix. This catches browser helper processes.
2. Build the tap description:
   - Matches found: `CATapDescription(stereoMixdownOfProcesses: ids)`.
   - None found: `CATapDescription(stereoGlobalTapButExcludeProcesses: [])`.
   - Then set `uuid = new`, `name = "JezFlow meeting"`, `isPrivate = true`, `muteBehavior = .unmuted`.
3. `AudioHardwareCreateProcessTap(desc, &tapID)`.
4. `AudioHardwareCreateAggregateDevice` with:
   - name `"JezFlow Meeting"`
   - UID `"com.jez.flow.meeting.<UUID>"`
   - `IsPrivate: true`, `IsStacked: false`, `TapAutoStart: true`
   - `SubDeviceList: [{SubDeviceUID: <default output device UID>}]`
   - `TapList: [{DriftCompensation: true, SubTapUID: desc.uuid}]`
5. Read `kAudioTapPropertyFormat` (an ASBD) into an `AVAudioFormat`, and create a Resampler from it.
6. `AudioDeviceCreateIOProcIDWithBlock` on the aggregate device. The callback wraps the input `AudioBufferList` (no copy), resamples it, and passes it to `onChunk`. Chunks are about 150 samples at 16 kHz.
7. `AudioDeviceStart`.
8. Add a listener on `kAudioHardwarePropertyDefaultOutputDevice`, delivered on the main queue. When the output changes, stop the tap and start it again. This handles AirPods connecting mid-call.

Stopping reverses the setup:
- Remove the listener.
- `AudioDeviceStop`
- `AudioDeviceDestroyIOProcID`
- `AudioHardwareDestroyAggregateDevice`
- `AudioHardwareDestroyProcessTap`

Errors use NSError code 7 with the message `"System audio: … failed (status)"`.

**Segmenter** (energy VAD; thread-safe through a lock)

| constant | value |
|---|---|
| frame | 320 samples (20 ms) |
| endSilence | 35 frames (700 ms) |
| maxSamples | 20 × 16000 (20 s) |
| initial noise | 0.004 |
| noise floor minimum | 0.002 |
| `speechFloor` | 0.008 default (0.010 for the tap) |
| `minSpeechFrames` | 12 default (15 for the tap) |
| pre-roll | 4800 samples (300 ms) |

For each 20 ms frame:
1. Compute `rms`.
2. If `rms < noise * 1.5`, update `noise = max(0.002, noise * 0.995 + rms * 0.005)`.
3. `speech = rms > max(speechFloor, noise * 4)`.
4. Append the frame to the buffer and add 320 to `elapsed`.
5. A speech frame increments `speechFrames` and resets `silentFrames`; a quiet frame increments `silentFrames`.
6. If `speechFrames > 0` and (`silentFrames >= 35` or the buffer holds at least 20 s), flush.
7. Otherwise, if `speechFrames == 0` and the buffer holds more than 4800 samples, trim it to the last 4800.

**Flush:**
- If `speechFrames >= minSpeechFrames`, emit `(buffer, max(0, (elapsed - buffer.count) / 16000))`.
- Always clear the buffer and both counters.
- The emitted chunk includes the trailing 700 ms of silence and the pre-roll.

`finish()` flushes whatever is left.

## 8. WhisperServer (whisper.cpp)

- **Port 8768**, host 127.0.0.1.
- `available` = both the binary at `whisperServerPath` and `models/{whisperModel}` exist.
- **Start:**
  1. Run `/usr/bin/pkill -f "whisper-server.*--port 8768"` and wait for it to exit.
  2. Launch `{whisperServerPath} -m {modelPath} --host 127.0.0.1 --port 8768 -l auto -t 6` with stdout and stderr sent to null.
  3. Readiness: `GET http://127.0.0.1:8768/` with a 1 s timeout. Any HTTP response counts. Poll every 500 ms for up to 60 s, otherwise throw "whisper-server didn't start".
- **Stop:** `process.terminate()`. The server runs only for the length of a meeting and uses about 2 GB.
- **Request:** `POST /inference`
  - multipart/form-data, boundary `jezflow-<UUID>`
  - fields `response_format=verbose_json`, `temperature=0.0`, `temperature_inc=0.2`
  - `file` = `audio.wav` (`audio/wav`)
  - timeout 60 s
- **Response:** uses `text` (required), `language` (for example `"english"`, lowercased) and `segments[]`.
- **Post-processing:**
  1. Replace `\n` with a space and collapse repeated whitespace.
  2. Run `TextRules.filterWhisper(text, segments, seconds)`.
- Errors use NSError code 10.

**WAV encoding (`Cloud.wav`)**
- Samples are clamped to [-1, 1] and scaled by 32767 into Int16 LE.
- 44-byte RIFF header: PCM (format 1), 1 channel, 16000 Hz, byte rate 32000, block align 2, 16 bits.

**`filterWhisper`** drops a segment if any of these hold:
- `no_speech_prob > 0.6` and `avg_logprob < -0.7`
- `avg_logprob < -1.2`
- `compression_ratio > 2.4`
- its text (lowercased, with punctuation and whitespace trimmed) is a ghost phrase: thank you, thanks for watching, thank you for watching, bye, you, the end, subtitles by the amara.org community, please subscribe, like and subscribe, thank you very much, i'm sorry, so, oh, um

Missing fields default to `no_speech_prob` 0, `avg_logprob` 0 and `compression_ratio` 1. With no segments in the response, the whole text is checked against the ghost list. Finally, if the clip is longer than 1 s and the words per second exceed 6, return `""`.

**Groq cloud ASR**
- `POST https://api.groq.com/openai/v1/audio/transcriptions`
- Header `Authorization: Bearer <key>`
- Fields `model`, `response_format=verbose_json`, `temperature=0`, optional `language`, and the file. **No prompt field is sent**, even though the caller passes a prompt.
- Timeout 20 s. The response must be status 200 with a `text` field, and then goes through the same `filterWhisper`.
- LLM fallback models: `openai/gpt-oss-120b`, `openai/gpt-oss-20b`.

## 9. ModelCatalog and downloader

| file | label | URL | GB |
|---|---|---|---|
| `ggml-large-v3-turbo.bin` (`ModelCatalog.whisper`) | Whisper large v3 turbo | `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin` | 1.6 |
| `Qwen3.5-2B-Q4_K_M.gguf` | Qwen3.5 2B | `https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf` | 1.3 |
| `Qwen3.5-4B-Q4_K_M.gguf` (default) | Qwen3.5 4B | `.../unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf` | 2.7 |
| `Qwen3.5-9B-Q4_K_M.gguf` | Qwen3.5 9B | `.../unsloth/Qwen3.5-9B-GGUF/resolve/main/Qwen3.5-9B-Q4_K_M.gguf` | 5.7 |

Notes shown in the UI:
- Whisper: "Needs `brew install whisper-cpp`"
- 2B: about 0.4 s per dictation
- 4B: about 0.7 s, 3 GB RAM
- 9B: about 1.5 s, 6 GB RAM

**`ModelDownloader`**
- Uses a `URLSession` with `timeoutIntervalForRequest = 600`, `timeoutIntervalForResource = 86400` and `waitsForConnectivity = true`.
- Downloads are keyed by file name, taken from the URL's last path component. A key that is already downloading is ignored.
- Published state: `progress[file]` (0–1, only when the expected size is known) and `errors[file]`.
- On finish: remove any existing file at the destination, move the temp file into `models/` synchronously inside the delegate callback, then clear progress and set the error, if any.
- Cancelling removes the task and its progress. A cancellation error (`NSURLErrorCancelled`) is not reported.

## 10. NameSources (Contacts and Calendar)

- Runs at launch and then every **3600 s**, only when `useContacts` is on; otherwise it sets `store.externalNames = []`.
- The names only bias speech recognition and are never saved.

**Contacts**
- `requestAccess(.contacts)`.
- Enumerate contacts with the keys givenName, familyName, organizationName and nickname.
- Candidates per contact: given, family, nickname, organization, and "given family" joined.
- Keep a candidate if, after trimming, it has at least 3 characters and starts with a letter.

**Calendar**
- `requestFullAccessToEvents()`.
- Events from the start of today to **+48 h**, across all calendars.
- From each event, add `TextRules.contextNames(title)` (capitalised phrases of up to 3 words, 3+ characters each, excluding a common-word list, limit 15) and attendee names of at least 3 characters.

**Result:** deduplicated case-insensitively, keeping first-occurrence order.

## 11. FluidAudio usage (exact calls)

**`Transcriber` (actor, Parakeet TDT on CoreML; the same instance is shared for dictation and meetings)**
- Load:
  - `AsrModels.downloadAndLoad(version: v)`, where `v` is `AsrModelVersion` `.v3` if `"v3"`, otherwise `.v2`.
  - `AsrManager(config: .default)`
  - `try await m.loadModels(models)`
- Batch transcription:
  - `var state = try TdtDecoderState(decoderLayers: await manager.decoderLayerCount)`
  - `let result = try await manager.transcribe(samples, decoderState: &state)`
  - Then use `result.text` and `result.tokenTimings`.
  - If boosting is enabled: `await boosting.rescore(text:, tokenTimings:, audioSamples:)`. When the output has `wasModified`, use `out.text`.
  - Trim the result.
  - Throws code 2 "Speech model still loading" if the model isn't loaded.
- Vocabulary boosting (`setVocabulary(terms, extra)`):
  - Merge the dictionary terms with the extra names, deduplicating case-insensitively.
  - Trim each term and keep those with at least 4 characters. If the list is unchanged, do nothing.
  - `CtcModels.downloadAndLoad()` (parakeet-ctc-110m, loaded lazily).
  - `CustomVocabularyContext(terms: [CustomVocabularyTerm(text:)], minSimilarity: 0.75, minTermLength: 4)`
  - `VocabularyBoostingSession(vocabulary:, ctcModels:, config: .init(spotterRescueEnabled: false))`
  - At launch this is called only with `config.dictionary`.
- Live dictation (not used by meetings):
  - Freeze after 8 s of audio, at the quietest 200 ms window (3200 samples, step 800) between 5 s and 7.5 s.

**`MeetingEngine`**
- `SenseVoiceManager.load()` and `sense.transcribe(audio:)`, which returns a String.

## 12. Porting notes

- Timestamps come from the segmenter's sample count, not wall-clock time. The mic and tap clocks are independent, and both start at session start.
- A watchdog restart of the mic does not reset the segmenter's elapsed time.
- `Store.update` rewrites the whole JSON file on every change. The live UI reads `store.meetings`.
- Swift dates are stored as seconds since 2001-01-01, so the Rust port must convert them to stay compatible with existing JSON files.
## 13. Beyond the Swift app (OpenFlow changes)

These deliberately differ from the Swift behaviour above. Some were adopted from a review of the
Meeting-Assistant project; the others fix gaps that neither app handled.

- **One wall clock for both channels.** An utterance's `time` is its start in seconds since the session started,
  placed from when it was emitted (the segmenter reports how far behind "now" its end is). Mic restarts no longer
  shift the timeline, and "You" and "Them" lines can be compared.
- **Speaker echo is dropped** (`openflow_core::echo`). Without headphones the mic hears the far side. Each mic
  utterance waits (at most 30 s) until the tap has no utterance in progress that could overlap it. It is then
  dropped when at least 60% of its words (three or more words) were said by the far side within ±2 s.
- **Cap at 25 s, cut at a pause.** The segmenter's hard cap is 25 s, under Whisper's 30 s window. At the cap it
  cuts just after the quietest frame of the last 4 s when that frame is under half the stretch's average. The
  rest carries into the next utterance.
- **Whisper decoding.** `whisper-server` runs with `-nf` (no temperature fallback, the main source of fluent
  inventions on noise) and `-sns` (suppress non-speech tokens). Requests send `temperature_inc=0.0` and, when
  the dictionary has terms, `prompt="Vocabulary: a, b."` (capped at about 400 characters).
- **Stricter hallucination filter.** Segments with `no_speech_prob > 0.5` and `avg_logprob < -0.5` are dropped,
  and so are bare web addresses. Sound annotations (`[BLANK_AUDIO]`, `(music)`, `*sighs*`, `♪…♪`) are
  stripped. Decoder loops are collapsed: a repeated 3–10 word run is dropped, and one or two words repeated
  four or more times are cut back to one copy.
- **whisper-server lifecycle.**
  - Far-side lines wait for the server to finish starting, instead of going to Parakeet for the first minute.
  - A one-second silent warm-up absorbs the Metal compile.
  - A crashed server is restarted, at most twice per meeting.
  - The server is stopped only after the queue has drained.
- **Retry once.** A failed transcription is retried after 500 ms before the utterance is given up.
- **Tap health (helper).**
  - Every 2 s the tap is rebuilt after 8 s without an IO callback. A live tap delivers buffers even in silence.
  - It is also rebuilt when the call app's set of audio processes changes, because browsers and Zoom start new
    ones mid-call.
- **Auto-stop.** A meeting tied to a call app ends 20 s after that app closes its microphone
  (`kAudioProcessPropertyIsRunningInput`), but only once the mic has been seen open.
- **Crash recovery.** At launch, a meeting without an `end` is closed 5 s after its last line, and then gets notes.
- **Empty and tiny meetings.** A stopped meeting with no lines and no notes is deleted. Notes are not written
  for fewer than 25 words.

## 14. Speaker names and the voice gate

- **Silero VAD gate** (`MeetingEngine::has_voice`). Every utterance goes through Silero (FluidAudio Core ML,
  256 ms chunks, helper method `vad`) before speech recognition. The utterance is skipped when no chunk
  reaches 0.5. This is a gate only: the clip is never cut. If the VAD errors, the audio is kept (fail open).
- **Speaker timeline** (`openflow_core::speakers`). Evidence is stamped on the meeting clock.
  - A far-side line gets a name in one of two ways:
    - by caption words: at least 2 matching words, covering 40% of the line, and 1.5× the runner-up;
    - otherwise by the dominant active speaker over its audio interval: 0.3 s lag compensation, evidence stale
      after 10 s, at least 0.2 s of overlap, and 1.25× the runner-up.
  - Otherwise the line stays "Them".
  - The user's own name never labels a far-side line.
  - The notes prompt treats any label other than "You" as someone else on the call.
- **Zoom** (helper `zoomSpeakers`, polled every 1 s during a Zoom meeting). Accessibility with
  `AXManualAccessibility`. It reads the "Talking: X" label, the floating window's first tile, tile descriptions
  and profile buttons.
- **Google Meet** (`browser-extension/`, `src-tauri/src/bridge.rs`).
  - The extension POSTs `{"type":"speaker"|"caption"|"call", …}` to `http://127.0.0.1:8770/meet`.
  - The app accepts only `Origin: chrome-extension://mbddbmiendbipklcehlgelanomjpoppf`.
  - Bodies are capped at 64 KB, names at 60 characters and captions at 2000 characters.

## 15. Speech server: transcribe.cpp

- `sidecar/transcribe-server/` builds `transcribe-server`:
  - It wraps [transcribe.cpp](https://github.com/handy-computer/transcribe.cpp) v0.2.3, pinned in
    `PINNED_COMMIT`, and runs on Metal.
  - It is statically linked (about 4 MB) and bundled as a Tauri `externalBin`.
  - `build.sh` refuses to build any other commit.
- The model is `whisper-large-v3-turbo-Q8_0.gguf` (886,381,824 bytes). Its URL is revision-pinned, and the
  downloader checks its SHA-256.
- Configs still on the old Homebrew defaults (`/opt/homebrew/bin/whisper-server`, `ggml-large-v3-turbo.bin`) migrate
  on load. A custom path is kept; a path whose file name starts with `whisper-server` still gets whisper.cpp's flags.
- Protocol:
  - Same `POST /inference` multipart as whisper-server (`file`, `language`, `prompt`, `temperature`,
    `temperature_inc`).
  - The reply is `{text, language (ISO code), truncated, segments[{text, no_speech_prob, avg_logprob,
    compression_ratio}]}`. The segment fields come from transcribe.cpp's per-chunk Whisper trace.
  - Every request needs `Authorization: Bearer <OPENFLOW_STT_TOKEN>`, a random token per server start. A web page
    can POST to localhost, but it cannot read the token.
  - The server exits within 0.5 s once `OPENFLOW_PARENT_PID` is gone.
- Measured on this Mac with a 5 s clip:
  - Load: 7.5 s cold, 0.9 s warm.
  - About 0.35 s per utterance.
  - About 1.06 GB resident.
- `no_speech_prob` is real but near 0 for large-v3-turbo, even on silence (logit -13 against a top logit of 8.7).
  This is a property of the model, not of the engine. Silence and noise are therefore stopped by:
  - the Silero gate;
  - the ghost list;
  - the log-probability rules;
  - `echoes_prompt`, which drops output made only of the vocabulary prompt's words.
