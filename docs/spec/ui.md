> Reference spec of the legacy Swift app (now in `dep/`, git-ignored; absolute path `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/`). Where this spec and the Swift source disagree, the Swift source wins. Prompts, tone texts and regexes must be copied verbatim from the Swift source.

# JezFlow UI, state and tooling spec (for the Tauri + React port)

The port should start from `PORTING-PROMPT.md`. It already describes the whole app as a rebuild target for other platforms (see §9). It does not cover the UI in detail, and that is what this spec adds.

All source is in `/Users/himanshu/codes/JezFlow/dep/Sources/JezFlow/` unless noted.

---

## 1. App shell (`main.swift`, `AppDelegate.swift`, `Resources/Info.plist`)

**Info.plist**
- Bundle id `com.jez.flow`, name and display name `JezFlow`, executable `JezFlow`, icon `AppIcon`.
- Version `0.1.0`, build `1`, minimum macOS `15.0`, high-resolution capable.
- **There is no `LSUIElement` key.** At runtime the app calls `setActivationPolicy(.regular)`, so it shows in the Dock and in the menu bar.
- Usage descriptions (exact text):
  - Microphone: "JezFlow listens while you hold the hotkey so it can transcribe your dictation on-device."
  - Audio capture: "JezFlow captures the other side of a call so meeting notes include everyone, transcribed on-device."
  - Contacts: "Names from your contacts help JezFlow recognise who you are talking about. They stay on this Mac."
  - Calendars (full access): "Today's meeting titles and attendees help JezFlow recognise names. They stay on this Mac."

**Launch sequence**
1. Build the standard app menu, then the status-bar menu, then call `render()`.
2. Request microphone access. If denied, the state becomes `.error("Microphone permission denied")`.
3. Show the Accessibility prompt (`AXIsProcessTrustedWithOptions`).
4. Start the hotkey monitor: Cmd + key code 41 (`;`); Cmd+Shift uses the alternate tone; Esc cancels.
5. Wire the store's `startMeeting`, `stopMeeting` and `summarize` closures, plus the meeting detector and the overlay callbacks.
6. Refresh Contacts/Calendar names now and every 3600 s.
7. Load the transcriber, then start the polisher.
8. Open the window if `--settings` was passed or `config.windowAtLaunch` is true.
9. Clicking the Dock icon reopens the window when no window is visible.

**Main window** (`openSettings`)
- `NSWindow` 1080×700; titled, closable, resizable, miniaturizable, full-size content view.
- Title bar is transparent and the title is hidden. Background rgb(0.08, 0.08, 0.09). Dark Aqua appearance.
- `isReleasedWhenClosed = false`; the window is centred.
- `--settings <PageRawValue>` sets the starting page, e.g. `--settings Models`.
- It forces itself to the front with `orderFrontRegardless`, then activates the app.
- It logs the window rect so `screencapture -R` can grab it.

**Standard menu bar**
- App menu: About JezFlow; Settings… (⌘,); Hide JezFlow (⌘H); Quit JezFlow (⌘Q).
- Edit: Undo (⌘Z), Redo (⌘⇧Z), Cut, Copy, Paste, Select All.
- Window: Close (⌘W), Minimize (⌘M).

**Status-bar menu** (`buildMenu`)
- The status item is square length.
- Items, top to bottom:
  1. Status line (disabled item; starts as "Loading…").
  2. Separator.
  3. **Tone ▸** submenu.
  4. **History ▸** submenu.
  5. Separator.
  6. "Start Meeting Notes" / "Stop Meeting Notes" (runs `toggleMeeting`, which uses `detector.currentCall()` as the app).
  7. "Open JezFlow…" (⌘,).
  8. Separator.
  9. "Quit JezFlow" (⌘Q).
- The menu delegate's `menuWillOpen` rebuilds the Tone and History submenus every time the menu opens.
- **Tone submenu:**
  - "Automatic (per app)", checked when `toneOverride == nil`.
  - Separator.
  - One item per `config.tones.keys.sorted()`, titled with the name capitalised, checked when it is the override.
  - The override is sticky for the session only and is not saved.
- **History submenu:**
  - Empty history shows a disabled "No dictations yet".
  - Otherwise it lists the first 15 dictations: text on one line, cut to 70 characters plus "…", full text as the tooltip.
  - Items 1–9 get the key equivalents "1"…"9" with no modifier.
  - Clicking an item copies its `text` (not the correction) and sets the status line to "Copied to clipboard".
  - Then a separator and "Clear History" (`store.clearHistory()`).

**Status line and icon per state** (`render()`; the same text is also written to `store.status`)

| State | SF Symbol | Text |
|---|---|---|
| loading | hourglass | "Loading models…" |
| idle, hotkey not running | mic | "Grant Accessibility to enable the hotkey" |
| idle, no polisher | mic | "Ready (verbatim only — polish model not loaded)" |
| idle | mic | "Ready — hold ⌘; to dictate" |
| recording | mic.fill | "Recording (tap ⌘; to stop, Esc to cancel)" in toggle mode; otherwise "Recording (release to finish)" |
| processing | ellipsis.circle | "Transcribing…" |
| error(m) | exclamationmark.triangle | "Error: m" |

**Overlay (floating pill)**
- Only shown when `config.overlay` is on.
- recording → `.listening`; processing → `.processing`; error → `.message(m)`.
- Otherwise: `.meetingLive(start)` during a meeting, `.meetingOffer(app)` when a call is detected, else hidden.
- A meeting offer expires after 45 s.

**Hotkey flow**
- A press shorter than `tapThreshold` switches to toggle mode; a hold ends recording on release.
- Recordings under 0.25 s are dropped.
- If the peak level is 1e-5 or lower, the error is "Microphone gave silence: pick another input in Settings (built-in is off while the lid is closed)". It clears after 5 s.
- Live decoding runs every ~400 ms (never faster than every 120 ms).
- A dictation is "long" if it is over 15 s or contains a correction signal. Long dictations go to the cloud when `cloudDictation == "long"`.
- Cloud speech and the local final decode run in parallel; if the cloud fails, the pill shows "Cloud unavailable, used local models".
- Other error states clear after 4 s.
- Tone resolution order: `toneOverride`, then `appTones[bundle]`, then `defaultTones`. Cmd+Shift picks `.alt`.

**Meetings (AppDelegate)**
- Default title: `"<App> call · EEE d MMM, h:mm a"`, or `"Meeting · …"` when no app is known.
- Whisper server: started when `meetingFarEngine == "whisper"` and the cloud is off.
- Notes refresh every 180 s while live, but only if at least 40 new words have arrived.
- If system audio is not captured, the pill shows "Only your mic is captured: allow System Audio Recording".
- If starting fails, the meeting is deleted and the pill shows "Meeting notes failed: …".
- Summarising: if the cloud is off and `notesModel` exists and differs from the polish model, a second llama-server starts on **port 8767** (ready timeout 180 s) and is stopped afterwards.
- The summary and title go through Chinese-script conversion. The title is only rewritten while `titleAuto` is true.
- After the polisher starts, every finished meeting without notes is summarised, one at a time.

---

## 2. Theme (`Theme.swift`): exact values

**Colours** (sRGB, 0–1)

| Token | Value | Approx. hex |
|---|---|---|
| window | 0.08, 0.08, 0.09 | #141417 |
| card | 0.113, 0.113, 0.125 | #1D1D20 |
| row | 0.145, 0.145, 0.16 | #252529 |
| rowHover | 0.175, 0.175, 0.19 | #2D2D30 |
| border | white @ 0.08 | |
| text | white 0.92 | #EBEBEB |
| secondary | white 0.58 | #949494 |
| tertiary | white 0.40 | #666666 |
| accent | 0.62, 0.55, 0.98 | #9E8CFA |
| accentSoft | accent @ 0.18 | |
| navSelected | white @ 0.09 | |
| destructive text | 1, 0.45, 0.45 | |
| "Them" speaker | 0.4, 0.75, 0.6 | |

- Main radius (`Theme.radius`) is 16. The app forces a dark colour scheme.
- The live badge uses red; status dots use green, yellow and gray. Error text is red; warning text is orange.

**Shared components**
- **PageHeader:** title 30pt semibold (text colour); optional subtitle 13pt (secondary), 4pt below; optional trailing view on the right; 22pt bottom padding.
- **Tabs:** HStack with 26pt spacing. Label is 15pt, semibold when selected (text colour) and regular otherwise (secondary). The selected tab has a 2pt underline in text colour, 10pt below the label. A 1pt border line runs under the whole strip; 18pt bottom padding.
- **PrimaryButton:** 14pt semibold black text on a text-colour fill, padding 18h×10v, radius 10.
- **SecondaryButton:** 13pt medium, text colour (or the destructive red); row fill, 1pt border, padding 14h×8v, radius 9.
- **IconButton:** SF Symbol 13pt medium in secondary colour, 28×28 hit area, tooltip from `help`.
- **RowCard:** HStack spacing 12, padding 18h×14v. Background is row, rowHover on hover. The trailing actions have opacity 0 until hovered.
- **CardList:** VStack with spacing 0, row fill, clipped to radius 12, 1pt border. Rows are separated by 1pt border-colour lines.
- **StatTile:** value 34pt semibold; label uppercase 11pt medium, tracking 1.2, secondary; extra content under it. Padding 22, row fill, radius 14, border.
- **Field style (`.field()`):** 14pt, padding 12h×9v, window-colour fill, radius 9, border.
- **SectionLabel:** uppercase 11pt semibold, tracking 1.2, secondary.
- **Helpers:** `appDisplayName(bundle)` returns the app name without ".app", falling back to the last component of the bundle id. `appIcon(bundle)` returns the app icon.
- Fonts are the system font throughout. The Home stat numbers use the serif design; the tone editor uses monospaced.

---

## 3. Main window layout (`MainView.swift`)

- Minimum size 960×620; window-colour background.
- Sidebar is 220 wide. The content card has radius 16, card fill and a border; its inner padding is 36. The card has 14pt padding on the right and bottom and 8pt on top.
- **Sidebar:**
  - Brand row: waveform icon 20pt semibold, "JezFlow" 22pt semibold, and a "Local" pill (12pt semibold accent on accentSoft, padding 8×3, radius 6). Padding: 22 left, 44 top, 26 bottom.
  - Nav order: Home (mic), Meetings (person.2.wave.2), Insights (chart.bar), Dictionary (character.book.closed), Style (textformat), Memory (brain), Models (cpu).
  - Then a spacer, a 1pt divider, Settings (gearshape), and **Help** (questionmark.circle). Help opens `README.md` from the app bundle, not the guide.
  - Nav item: icon 16pt medium in a 22-wide frame, label 16pt medium, padding 12×10, radius 10; `navSelected` fill when selected.
  - Status footer: a 7pt dot (green when speech and polish are both ready, yellow when only speech is ready, gray otherwise) and `store.status` at 11pt secondary, up to 2 lines.

---

## 4. Pages

### Home (`HomePage`)
- **Layout:** HStack with 28pt spacing. Left column: the history. Right column: 250 wide, 74pt top padding, holding a stats panel and an engines panel.
- **Header:** "Dictation", subtitle "Hold ⌘; anywhere to talk. Everything below stays on this Mac."
- **Search:** magnifying-glass field "Search dictations"; row fill, radius 9, padding 12×8. Matches `text` case-insensitively.
- **Empty states:** "No dictations yet. Hold ⌘; and say something." when there is no history; "No matches." when the search finds nothing.
- **Grouping:** by day, labelled "Today", "Yesterday" or "EEEE d MMM", shown as a SectionLabel over a CardList.
- **HistoryRow:**
  - Time "h:mm a" (13pt secondary, 70 wide).
  - Shown text is `correction ?? text` (15pt, selectable).
  - Meta line (11pt tertiary): app name · tone · "Ns" · "corrected" (accent) · waveform icon when a clip exists.
  - Hover actions:
    - pencil ("Correct: save what this should have said") opens the editor;
    - copy (the icon becomes a checkmark for 1.5 s);
    - trash (`store.delete`).
  - Editor: "Heard: raw" (12pt tertiary), a TextEditor (minimum height 56, window fill, radius 8), **Save correction** (runs `store.setCorrection`) and **Cancel**.
- **Stats panel:** padding 22, radius 14. Three rows of value (28pt medium serif) plus label (15pt secondary): "total words", "wpm", "day streak". Both branches of the streak label say "day streak", so the singular/plural switch does nothing.
- **Engines panel:** "Engines" 16pt semibold, then two lines, each with a green/gray dot:
  - "Parakeet {asrVersion}" (dot follows `speechReady`);
  - the model file name without ".gguf" (dot follows `polishReady`).

### Meetings (`MeetingsPage`)
- **Header:** "Meetings", subtitle "Beta. Transcribed and summarised locally (or via your own cloud key). Audio is never stored."
  - Trailing button: **Stop** (destructive; runs `store.stopMeeting`) while a meeting is live, otherwise **Start notes now** (runs `store.startMeeting(nil)`).
- **Empty state:** "No meetings yet. JezFlow offers to take notes when a call starts, or press Start notes now."
- **Left column** (240 wide):
  - Search field "Search meetings" with a clear (x) button.
  - Meeting list: title 13pt medium, LIVE badge when live, meta line "d MMM · N min · N words". Card radius 10; `navSelected` fill when selected.
- **Search results:**
  - Needs at least 2 characters. Searches Title, Summary, Notes and each transcript line ("speaker: text"), case- and diacritic-insensitive.
  - Up to 3 hits per meeting and 30 in total. Snippets include 60 characters either side, with "…" added where cut.
  - Results show as an overlay 44pt down: 520 wide, max height 360, card fill, radius 12, shadow (black @ 0.5, radius 18, y 8).
  - Each result: title, an accent field pill, the date, and the snippet with the match in bold.
  - Clicking selects the meeting, switches to the matching tab and clears the query.
- **Selection:** the chosen meeting, else the live meeting, else the first one.
- **Detail pane:**
  - Title is an editable TextField (20pt semibold). Editing sets `title` and `titleAuto = false`.
  - Tabs: Summary / Transcript / Notes.
  - Icons: copy (the current tab's content; for Summary it uses `NotesView.plainText`), export (`NSSavePanel`, "<title>.md" with `/` and `:` replaced by `-`, writes `m.markdown`), and trash (only when the meeting is not live).
  - **Summary tab:**
    - Empty summary: a spinner with "Writing the notes…", or "Listening… notes appear a few minutes in." / "Nothing was transcribed." / "Notes are on their way."
    - Otherwise NotesView, followed by "Updating…" (spinner), "Updates every few minutes while the meeting runs." (while live), or a **Rewrite notes** button (runs `store.summarize`).
    - Notes are generated automatically when a finished meeting that has segments but no summary is shown. This happens at most once per meeting per view.
  - **Notes tab:** TextEditor bound to `meeting.notes`, row fill, radius 10.
  - **Transcript tab:**
    - Lines sorted by time. Each line: clock (11pt monospaced digits, 44 wide, right-aligned), speaker (12pt semibold, 40 wide; "You" in accent, others green), text 14pt.
    - Auto-scrolls to the bottom while live.
    - Empty: "Listening… lines appear as each person finishes a sentence."
- **NotesView:**
  - Parses `#` headings and `-`, `•` or `*` bullets; drops items that just say "none".
  - Section title 17pt semibold; bullet "•" in accent; item text 14pt.
  - The "Your to-dos" section renders `meeting.todos` as checkboxes. Checked items are struck through in tertiary.
  - Inline markdown is rendered, and a leading "Name:" is made bold.
- **LiveBadge:** red 6pt dot and "LIVE" (10pt bold) on red @ 0.15, capsule.

### Insights (`InsightsPage`)
- **Header:** "Insights", subtitle "All-time, counted locally since you installed JezFlow."
- **Stat tiles:**
  - WPM, with "Speaking is ~3× faster than typing";
  - total words, with "{dictations} dictations";
  - day streak, with "{activeDays.count} active days".
- **Two usage panels:** "App usage" (`perApp`) and "Tone usage" (`perTone`, names capitalised).
  - Each shows the top 8 rows: name (110 wide), a 26pt-high bar (window-colour track, accent @ 0.75 fill, minimum width 4, "%" label in white 11pt semibold), and the count (40 wide).
  - Panel title 20pt semibold. Empty panel: "Nothing yet."

### Dictionary (`DictionaryPage`)
- **Header:** "Dictionary", subtitle "Names and jargon spelled exactly like this. Sound-alikes in the transcript get replaced." with an **Add new** button.
- **Add row:** field "Term, or Term: sound-alike, sound-alike (e.g. Zentro: zentrow, zen tro)" with **Save** and **Cancel**.
  - Adding inserts the term at index 0 of `config.dictionary`, skipping duplicates.
- **List:** CardList of terms, with a trash button on hover. Empty: "No terms yet."

### Style (`StylePage`)
- **Header:** "Style", subtitle "How your dictation reads, and which style each app gets." The **Add tone** button shows only on the Tones tab.
- **Tabs:** Tones / Apps.
- **Tones tab:**
  - Adaptive grid (minimum column 200, spacing 14) of cards, each 130 high, padding 18, radius 14. The border is 2pt accent when selected, 1pt otherwise.
  - Card: name capitalised (17pt semibold), a Default or Alt badge, and the instruction (12pt, 4 lines). An empty instruction shows "Verbatim — pasted exactly as transcribed."
  - Editor below the grid:
    - Name field (220 wide). Submitting renames the tone (lowercased; also updates `defaultTones` and every `appTones` entry).
    - **Set as default** (`defaultTones.tone`), **Set as alt (⌘⇧;)** (`defaultTones.alt`), and **Delete** (destructive).
    - Helper text: "Instruction given to the polish model after the cleanup rules. Leave empty for verbatim."
    - Monospaced 13pt TextEditor (minimum height 200) bound to `tones[name]`; every keystroke saves.
  - **Add tone** creates "new tone" (or "new tone N") with the instruction "Tone: ".
- **Apps tab:**
  - Helper: "⌘; uses the app's tone, ⌘⇧; its alt. Apps not listed use the default."
  - DEFAULT section: "All other apps" with two pickers (tone, then "alt") bound to `defaultTones`.
  - PER APP section: rows sorted by bundle id, each with a 26pt icon, the name, the bundle id (11pt tertiary), the two pickers bound to `appTones[bundle]`, and a remove button on hover.
  - Add row: a "Running app…" menu (regular apps, excluding `com.jez.flow`, sorted by name), a bundle-id field and an **Add** button. New entries copy `defaultTones`.
  - Each picker is 130 wide.

### Memory (`MemoryPage`)
- **Header:** "Memory", subtitle "Background the polish model always knows about you. Used for context only, never inserted into your text."
- **Editor:** TextEditor bound to `config.memory` (14pt, padding 12, row fill, radius 12).
- **Example text:** "Example: \"I'm Taylor, based in Melbourne. I run a small design studio called Acme Studio and I use Australian spelling.\""

### Models (`ModelsPage`)
- **Header:** "Models", subtitle "Everything runs on this Mac. Switching restarts the local server in a few seconds." with an **Open models folder** button.
- **POLISH MODEL** (`ModelCatalog.all`):

  | Label | File | Size | Note |
  |---|---|---|---|
  | Qwen3.5 2B | `Qwen3.5-2B-Q4_K_M.gguf` | 1.3 GB | "Fastest (~0.4 s)…" |
  | Qwen3.5 4B | `Qwen3.5-4B-Q4_K_M.gguf` | 2.7 GB | "Default. ~0.7 s…" |
  | Qwen3.5 9B | `Qwen3.5-9B-Q4_K_M.gguf` | 5.7 GB | "Most accurate…" |

  - Files come from `huggingface.co/unsloth/…-GGUF`.
  - Row states: downloading (ProgressView 120 wide, "N%", **Cancel**); installed (an **Active** capsule badge, or a **Use** button that calls `store.setModel`); missing (**Download**). Errors show in red.
  - Any other installed `.gguf` files get extra rows.
- **FAR SIDE OF CALLS:**
  - Whisper large v3 turbo, `ggml-large-v3-turbo.bin`, 1.6 GB, from `huggingface.co/ggerganov/whisper.cpp`.
  - Shows "whisper-server found" if `whisperServerPath` exists, otherwise "whisper-server not found: run brew install whisper-cpp" in orange.
- **SPEECH MODEL:** "Parakeet TDT 0.6B" with a picker bound to `asrVersion` through `store.setAsrVersion`: "v2 English" or "v3 Multilingual" (160 wide).

### Settings (`SettingsPage`)
Every row is a RowCard: title 15pt, subtitle 12pt tertiary (2 lines), control on the right. Switches are tinted accent. Unless noted, controls use `store.bind(\.x)`, which saves on every change.

| Section | Row (title) | Control → field |
|---|---|---|
| Hotkey | Dictate | keycap "⌘ ;" (display only) |
| | Alternate tone | keycap "⌘ ⇧ ;" |
| | Tap threshold | slider 0.2–1.0, step 0.05, 160 wide, "%.2f s" → `tapThreshold` |
| App | Open window at launch | switch → `windowAtLaunch` |
| | Launch at login | switch → `SMAppService.mainApp` register/unregister (not in config) |
| | llama-server | text field 260 wide → `llamaServerPath` |
| | Config folder (subtitle is the path) | **Open** button |
| Microphone | Input device | picker 230 wide: System default `""`, Built-in `"builtin"`, other devices by UID → `microphone` |
| While dictating | Floating pill | switch → `overlay` |
| | Sounds | switch → `sounds` |
| | Live text in the field | switch → `liveText` |
| | Read the field you're typing in | switch → `fieldContext` |
| Processing | Use cloud models (Groq) | switch → `cloudEnabled` |
| | Cloud for dictation | picker Off / Long dictations only / All (`off` / `long` / `all`) → `cloudDictation` |
| | Cloud for meetings | switch → `cloudMeetings` |
| | Groq API key | secure field "gsk_…" → `cloudAPIKey` |
| | Cloud speech model | text field → `cloudASRModel` |
| | Cloud text model | text field → `cloudLLMModel` |
| Meetings | Detect calls | switch → `meetingDetect` |
| | Chinese script | picker Traditional / As detected (Simplified) → `chineseScript` |
| | Far side (them) | picker `whisper` / `parakeet` → `meetingFarEngine` |
| | Notes model | picker: "Same as polish model" `""` plus installed GGUFs other than `model` → `notesModel` |
| | Your side (you) | picker Auto (any language) `auto` / English only `english` → `meetingEngine` |
| Names | Use Contacts and Calendar | switch → `useContacts` |
| | Learned automatically | "{dictionary.count} terms" (display only) |
| Tuning | Keep audio clips | switch → `keepClips` |
| | Corrections ("{correctionCount} saved · …") | **Copy eval command**, which copies `/Applications/JezFlow.app/Contents/MacOS/JezFlow --eval` |

- Picker widths are 190 unless noted.
- Keycaps: 13pt medium, padding 10×5, window-colour fill, radius 7.
- Subtitle text worth porting exactly:
  - Tap threshold: "Presses shorter than this switch to toggle mode".
  - Input device: "The built-in mic is silent while the lid is closed; AirPods are fine but narrowband".
  - Use cloud models: "Whisper large v3 turbo for speech, Llama 3.3 70B for cleanup and notes. Audio and text leave this Mac; live text stays local; falls back to local on failure". This mentions Llama 3.3 70B, but the default `cloudLLMModel` is `qwen/qwen3.8-27b`, so the text is out of date.
  - Cloud for dictation: "Long = dictations over 15 s, where the local 4B takes 5-7 s to polish; short ones stay local (as accurate, better on names, private)".

---

## 5. Config (`Config.swift`)

**Files**
- Folder: `~/Library/Application Support/JezFlow/`
- `config.json`: pretty-printed with sorted keys. If it is missing on load, the defaults are written first.
- `models/`: created on load.
- `modelURL` is `models/<model>`.

**Fields** (every one is decoded with `decodeIfPresent`, falling back to the default)

| Field | Type | Default |
|---|---|---|
| asrVersion | String | "v2" ("v3" = multilingual) |
| llamaServerPath | String | "/opt/homebrew/bin/llama-server" |
| model | String | "Qwen3.5-4B-Q4_K_M.gguf" |
| llamaPort | Int | 8765 |
| microphone | String | "" (or "builtin", or a device UID) |
| hotkeyKeyCode | UInt16 | 41 (`;`) |
| tapThreshold | Double | 0.5 |
| defaultTones | {tone, alt} | {supercasual, professional} |
| appTones | [bundle: {tone, alt}] | see below |
| tones | [name: instruction] | supercasual, casual, professional, verbatim ("") |
| dictionary | [String] | ["JezFlow: jez flow, jazz flow, jazz flo", "no wait: no wake, no weight, know wait"] |
| learnedSignals | [String] | [] (capped at 20) |
| memory | String | "" |
| keepClips / overlay / sounds / fieldContext / liveText / meetingDetect | Bool | true |
| meetingEngine | String | "auto" (the code comment says SenseVoice) or "english" |
| chineseScript | String | "traditional" |
| useContacts | Bool | false |
| windowAtLaunch | Bool | true |
| cloudEnabled | Bool | false |
| cloudDictation | String | "long" |
| cloudMeetings | Bool | true |
| liveTextExcluded | [String] | Terminal, iTerm2, Warp, kitty, Alacritty, Ghostty, WezTerm, Hyper, VSCode |
| cloudAPIKey | String | "" |
| cloudASRModel | String | "whisper-large-v3-turbo" |
| cloudLLMModel | String | "qwen/qwen3.8-27b" |
| whisperServerPath | String | "/opt/homebrew/bin/whisper-server" |
| whisperModel | String | "ggml-large-v3-turbo.bin" |
| meetingFarEngine | String | "whisper" |
| notesModel | String | "Qwen3.5-9B-Q4_K_M.gguf" |

**Default `appTones`**
- supercasual / professional: MobileSMS, WhatsApp, Slack.
- professional / casual: Mail, Chrome, Safari.
- verbatim / casual: Terminal, iTerm2, VSCode.

**Default tone text**
- The full instruction strings with examples are in `Config.swift` lines 36–41. Copy them word for word.
- They differ from the versions in `PORTING-PROMPT.md` and probably from `/Users/himanshu/codes/JezFlow/dep/starter-config.json`, which the guide recommends copying into place before first launch. The per-app defaults differ too: the porting prompt sets Safari, Terminal, iTerm2 and VSCode to supercasual.

**Migrations**
- `cloudDictation` stored as a Bool becomes "all" (true) or "off" (false).
- `Config.tones(for:)` returns `appTones[bundle]`, falling back to `defaultTones`.
- If `config.json` can't be parsed, the Store copies it to `config.json.bak` and uses the defaults.

---

## 6. Store (`Store.swift`)

**Published state:** `config`, `history` ([Dictation]), `stats`, `status` (starts "Loading…"), `externalNames`, `meetings` (`Meeting.loadAll()`), `liveMeetingID`, `summarizing` (Set<UUID>), `speechReady`, `polishReady`.

**Callbacks** (set by AppDelegate): `startMeeting(String?)`, `stopMeeting()`, `summarize(UUID)`, `onModelChange`, `onAsrChange`.

**Files**

| File | Contents |
|---|---|
| `history.json` | [Dictation], capped at 200 |
| `stats.json` | Stats |
| `clips/<uuid>.wav` | 16 kHz audio per dictation |
| `meetings/<uuid>.json` | one Meeting per file (`Meeting.swift`) |

**Data types**
- **Dictation:** id, date, text, raw, app?, tone, seconds, clip?, correction?. Computed: `words`, `hasClip` (the file exists).
- **Stats:** words, seconds, dictations, activeDays ["yyyy-MM-dd"], perApp, perTone.
  - `wordsPerMinute` = words ÷ (seconds ÷ 60).
  - `streak` counts consecutive days ending today, or ending yesterday if today has no activity yet.
- **Meeting:** id, title, titleAuto, todos [{id, text, done}], app?, start, end?, segments [{id, time, speaker, text}], summary, notes. Computed: `isLive` (end == nil), `duration`, `words`, `transcript`, `markdown`.

**Methods**
- `save()`: writes `config.json`.
- `bind(keyPath)`: a two-way binding that saves on every set.
- `remember(d)`: inserts at the front, trims to 200, updates stats (`perApp` uses "unknown" when there is no app), writes history and stats.
- `delete(d)`: removes the entry and its clip.
- `clearHistory()`: deletes `history.json` and the whole `clips/` folder. Stats are kept.
- `setCorrection(d, text)`:
  - An empty or unchanged correction clears it (nil).
  - Otherwise `TextRules.learnSignals` adds new phrases to `learnedSignals` (last 20 kept), and `learnNames` plus `mergeDictionary` add learned names to the dictionary.
- `corrections(tone, limit 5)`: the latest corrected `(raw, correction)` pairs for that tone, oldest first.
- `allCorrections(limit 3)`: the same, keyed by tone. These become the polish prompt's examples.
- `correctionCount`.
- Meetings: `addMeeting`, `update(id, change)` (saves the file), `appendSegment`, `finishMeeting` (sets `end` and clears `liveMeetingID`), `deleteMeeting` (removes the file).
- `setModel(file)` and `setAsrVersion(v)`: save, then call the change callback, which restarts the polisher or transcriber.
- `installedModels`: `*.gguf` files in `models/`, sorted.
- On first load, an old history saved as plain strings is converted to Dictation records.

---

## 7. CLI flags (`main.swift`) and test harnesses

| Flag | Behaviour |
|---|---|
| `--selftest` | `SelfTest.run()`: about 120 rule checks with no models, plus a benchmark ("rules: X ms per 60-word dictation"). Exit code 0 means all passed. |
| `--context` | Prints Accessibility trust, window title, and before/after-cursor character counts |
| `--livetest clip.wav [--target bundle]` | Streams the WAV in 500 ms steps every 350 ms, types into TextEdit only while it is frontmost, then swaps in the final text (professional tone) |
| `--polishtest "raw"` | Runs the polish step twice per tone and prints timings. Env vars: `JEZ_TONES_JSON`, `JEZ_FORCE_CLOUD`, `JEZ_TONES` (default casual,professional). Needs a running llama-server. |
| `--dumpprompt <tone>` | Prints the system prompt and the user message for a placeholder "TRANSCRIPT" |
| `--enginetest` | 8 s AVAudioEngine capture, prints the peak per second |
| `--mictest [--default] [--seconds=N] [--gap=N]` | Two recording rounds; reports samples, peak, and time to the first buffer |
| `--notestest` | Runs summary, to-dos and title on a built-in sample transcript |
| `--compare [N=20]` | Local vs Groq on the newest N clips: shows differences and average timings |
| `--tapprocs <name>` | Counts the audio processes matching the name |
| `--taptest` | 8 s system-audio tap while `say -v Samantha` speaks; passes if audio arrived with peak > 0.01 |
| `--eval [--no-boost]` | Replays corrected clips and prints a table of # / tone / WER for heard, shipped and now, marks exact matches with `*`, then lists the misses |
| `--settings [Page]` | Normal launch that opens the window, optionally on a given page |

The SelfTest groups are the port's conformance suite. They cover: fillers and stutters, spoken commands (new line, bullets, email, URL, punctuation words), the faithfulness guard, scratch-that, restarts, same-class swaps, ordinal and counted lists, the Segmenter, numbers (`applyNumbers`), dictionary aliases and learning, context names, meeting to-dos, `meetingLine`, `looksEnglish`, `filterWhisper`, `learnSignals`, Chinese conversion, emoji, casual/supercasual, greeting and message layout, question marks, `stripEcho`, and WER.

---

## 8. Other files

- **`scripts/make-guide.js`:** Node plus the `docx` package. Writes `JezFlow-Guide.docx` to `argv[2]`. It is the end-user guide: requirements, install steps (Homebrew, llama.cpp, `build.sh --install`, starter-config), tones, names, spoken commands, live text, cloud, Whisper, meetings, troubleshooting (including `log stream --predicate 'subsystem == "com.jez.flow"'`), privacy and storage, and appendices.
- **`.gitignore`:** `.build/`, `dist/`, `.DS_Store`, `Resources/AppIcon-personal.icns`.
- **`NOTICE.md`:** PolyForm Noncommercial 1.0.0. Credits FluidAudio (Apache-2.0), llama.cpp and whisper.cpp (MIT), Parakeet TDT and CTC 110M (CC-BY-4.0), Qwen3.5 GGUF from Unsloth (Apache-2.0), Whisper large v3 turbo (MIT) and SenseVoiceSmall. Groq is optional and uses the user's own key.

## 9. `PORTING-PROMPT.md` summary
It is a prompt to paste into an AI assistant so it can rebuild JezFlow on Windows, Linux or Intel macOS. It names no specific toolkit, so it doesn't mention Tauri. It covers:

1. **The experience:** Ctrl+; on Windows/Linux, hold-to-talk or tap-to-toggle, Esc cancels, a pill, and a settings window with Home/Dictionary/Style/Memory/Models/Settings. It does not mention the Meetings and Insights pages.
2. **Architecture:** 20 s warm microphone; Parakeet through sherpa-onnx with faster-whisper as fallback; live re-decoding every 350 ms that freezes text at 8 s; hotwords; llama-server with Qwen3.5-4B at temperature 0, `cache_prompt`, `--spec-type ngram-mod`; paste with the clipboard restored afterwards.
3. **Prompt design:** the system prompt stays byte-identical between dictations; the user message is a `[tone | window | before | after]` header line plus the transcript; the last 160 characters before the cursor and 100 after.
4. **The rules layer, in order.** Its output guard falls back to the rule-cleaned text when more than 30% of the long words are new; messages of 4 words or fewer in a casual tone skip the model.
5. **Tone text and per-app defaults.** These differ from the defaults in `Config.swift`.
6. **Learning:** corrections, name and signal learning, the eval tool.
7. **Groq cloud:** segment filter thresholds, fallback to `gpt-oss-120b`, 900 max tokens, the 15 s long-dictation rule.
8. **Meetings:** detection, WASAPI or PipeWire loopback, energy-gate parameters, merging same-speaker lines within 1.5 s, notes written in ~1300-word chunks, a 3–7 word title.
9. **Deliverables:** 14 conformance cases, and latency targets under 300 ms for the transcript and under 1.5 s for cleanup.