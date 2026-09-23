// In-memory stand-in for the Rust side, used only when the UI runs in a plain browser (`pnpm dev`).
// Implements every command in api.ts by name and emits the same events, with realistic sample data.

import type { EventCallback, UnlistenFn } from "@tauri-apps/api/event";
import starter from "../../starter-config.json";
import type {
  AppInfo,
  CatalogModel,
  Config,
  Dictation,
  DownloadProgress,
  InputDevice,
  Meeting,
  OverlayMode,
  OverlayState,
  Snapshot,
} from "./api";
import { APPLE_EPOCH_OFFSET } from "./api";

const nowApple = () => Date.now() / 1000 - APPLE_EPOCH_OFFSET;
const uuid = () => crypto.randomUUID().toUpperCase();
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v));

// ------------------------------------------------------------------------------------------------ events

const listeners = new Map<string, Set<EventCallback<unknown>>>();
let eventId = 0;

export function mockListen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  let set = listeners.get(event);
  if (!set) listeners.set(event, (set = new Set()));
  set.add(handler as EventCallback<unknown>);
  return Promise.resolve(() => set!.delete(handler as EventCallback<unknown>));
}

function emit(event: string, payload: unknown) {
  for (const h of listeners.get(event) ?? []) h({ event: event as never, id: ++eventId, payload: clone(payload) });
}

// ------------------------------------------------------------------------------------------------ sample data

const APPS: Record<string, string> = {
  "com.apple.MobileSMS": "Messages",
  "com.apple.Safari": "Safari",
  "com.apple.Terminal": "Terminal",
  "com.apple.mail": "Mail",
  "com.google.Chrome": "Google Chrome",
  "com.googlecode.iterm2": "iTerm",
  "com.microsoft.VSCode": "Visual Studio Code",
  "com.tinyspeck.slackmacgap": "Slack",
  "net.whatsapp.WhatsApp": "WhatsApp",
  "us.zoom.xos": "zoom.us",
  "com.apple.Notes": "Notes",
  "com.linear": "Linear",
  "com.himanshuat.openflow": "OpenFlow",
};
const APP_COLORS = ["#34c759", "#0a84ff", "#5e5ce6", "#ff9f0a", "#ff375f", "#64d2ff", "#bf5af2", "#30d158"];

function iconFor(bundle: string): string {
  const name = APPS[bundle] ?? bundle;
  let h = 0;
  for (const c of bundle) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const color = APP_COLORS[h % APP_COLORS.length];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="4" y="4" width="56" height="56" rx="13" fill="${color}"/><text x="32" y="42" font-family="-apple-system,Helvetica" font-size="28" font-weight="600" fill="white" text-anchor="middle">${name[0].toUpperCase()}</text></svg>`;
  return "data:image/svg+xml;base64," + btoa(svg);
}

const DAY = 86400;
const t0 = nowApple();

function dictation(ago: number, text: string, app: string | undefined, tone: string, seconds: number, extra: Partial<Dictation> = {}): Dictation {
  return { id: uuid(), date: t0 - ago, text, raw: text.toLowerCase().replace(/[.,!?]/g, ""), app, tone, seconds, ...extra };
}

const history: Dictation[] = [
  dictation(240, "Sounds good, I'll send the deck over tonight.", "com.tinyspeck.slackmacgap", "supercasual", 3.2, {
    text: "sounds good, i'll send the deck over tonight",
    clip: "a.wav",
  }),
  dictation(1500, "Hi Priya,\n\nThanks for the quick turnaround on the contract. I've added two comments on section 4; otherwise we're good to sign.\n\nBest,\nSam", "com.apple.mail", "professional", 17.8, { clip: "b.wav" }),
  dictation(3600 * 3, "Can we move the OpenFlow sync to Thursday at 3?", "com.google.Chrome", "professional", 4.1, {
    raw: "can we move the jazz flow sync to thursday at three",
    text: "Can we move the jazz flow sync to Thursday at 3?",
    correction: "Can we move the OpenFlow sync to Thursday at 3?",
  }),
  dictation(DAY + 600, "git commit -m 'fix overlay sizing'", "com.apple.Terminal", "verbatim", 2.4),
  dictation(DAY + 5000, "haha yes, see you at 7", "com.apple.MobileSMS", "supercasual", 1.9),
  dictation(DAY * 3 + 100, "Reminder to renew the domain before the 30th and check the DNS records for the staging site.", "com.apple.Notes", "casual", 6.3),
];
history.forEach((d) => {
  if (d.correction === undefined) delete d.correction;
});

function seg(time: number, speaker: string, text: string) {
  return { id: uuid(), time, speaker, text };
}

const meetings: Meeting[] = [
  {
    id: uuid(),
    title: "Q4 roadmap with design",
    titleAuto: true,
    todos: [
      { id: uuid(), text: "Send Maya the updated onboarding flow by Friday", done: false },
      { id: uuid(), text: "Book a room for the usability sessions", done: true },
    ],
    app: "us.zoom.xos",
    start: t0 - 3 * 3600,
    end: t0 - 3 * 3600 + 34 * 60,
    segments: [
      seg(3, "You", "Hey Maya, thanks for jumping on. I wanted to go through the Q4 roadmap."),
      seg(9, "Them", "Sure. I have the onboarding flow mostly done, but the permissions screen is still rough."),
      seg(21, "You", "Okay, let's prioritise that. Can you have something by Friday?"),
      seg(28, "Them", "Friday works if we drop the animation for now."),
      seg(40, "You", "Fine by me. I'll book a room for the usability sessions next week."),
      seg(55, "Them", "Great. Also, we decided the meetings page ships as beta."),
    ],
    summary:
      "## Summary\n- Went through the **Q4 roadmap** with design.\n- Onboarding flow is mostly done; the permissions screen still needs work.\n- Meetings page ships as *beta*.\n\n## Decisions\n- Drop the onboarding animation for now.\n- Meetings page ships as beta.\n\n## Action items\n- Maya: finish the permissions screen by Friday\n- None\n\n## Your to-dos\n- Send Maya the updated onboarding flow by Friday\n- Book a room for the usability sessions",
    notes: "Ask about the dark-mode icons.",
  },
  {
    id: uuid(),
    title: "Meeting · Mon 22 Sep, 10:00 AM",
    titleAuto: true,
    todos: [],
    start: t0 - DAY - 7200,
    end: t0 - DAY - 7200 + 12 * 60,
    segments: [
      seg(2, "You", "Quick check-in on the café launch. Where are we with the espresso machine?"),
      seg(8, "Them", "It arrives Wednesday. The barista training is booked for Thursday."),
    ],
    summary: "",
    notes: "",
  },
];

const catalog: CatalogModel[] = [
  {
    file: "Qwen3.5-2B-Q4_K_M.gguf",
    label: "Qwen3.5 2B",
    url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main/Qwen3.5-2B-Q4_K_M.gguf",
    gb: 1.3,
    note: "Fastest (~0.4 s). Fine for light cleanup, weaker on self-corrections.",
  },
  {
    file: "Qwen3.5-4B-Q4_K_M.gguf",
    label: "Qwen3.5 4B",
    url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main/Qwen3.5-4B-Q4_K_M.gguf",
    gb: 2.7,
    note: "Default. ~0.7 s per dictation, ~3 GB RAM.",
  },
  {
    file: "Qwen3.5-9B-Q4_K_M.gguf",
    label: "Qwen3.5 9B",
    url: "https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/main/Qwen3.5-9B-Q4_K_M.gguf",
    gb: 5.7,
    note: "Most accurate. ~1.5 s per dictation, ~6 GB RAM.",
  },
];

const whisperCatalog: CatalogModel = {
  file: "ggml-large-v3-turbo.bin",
  label: "Whisper large v3 turbo",
  url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
  gb: 1.6,
  note: "Far side of calls: any language, best on compressed call audio. Needs `brew install whisper-cpp`.",
};

const today = new Date();
const dayKey = (offset: number) => {
  const d = new Date(today.getTime() - offset * DAY * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
};

const state: Snapshot = {
  config: { ...(starter as unknown as Config), dictionary: ["OpenFlow: open flow, open flo", ...starter.dictionary, "Priya", "Maya"] },
  history,
  stats: {
    words: 12843,
    seconds: 5540,
    dictations: 912,
    activeDays: [dayKey(0), dayKey(1), dayKey(2), dayKey(3), dayKey(5), dayKey(9)],
    perApp: {
      "com.tinyspeck.slackmacgap": 402,
      "com.apple.mail": 188,
      "com.google.Chrome": 131,
      "com.apple.MobileSMS": 97,
      "com.apple.Terminal": 51,
      "com.apple.Notes": 30,
      unknown: 13,
    },
    perTone: { supercasual: 520, professional: 301, casual: 60, verbatim: 31 },
  },
  wpm: 139,
  streak: 4,
  meetings,
  liveMeetingId: null,
  summarizing: [],
  status: "Ready — hold ⌘; to dictate",
  speechReady: true,
  polishReady: true,
  correctionCount: 1,
  clipsPresent: history.filter((d) => d.clip).map((d) => d.id),
  installedModels: ["Llama-3.2-3B-Instruct-Q4_K_M.gguf", "Qwen3.5-4B-Q4_K_M.gguf", "Qwen3.5-9B-Q4_K_M.gguf"],
  catalog,
  whisperCatalog,
  whisperServerFound: false,
  whisperModelInstalled: false,
  dataDir: "/Users/you/Library/Application Support/OpenFlow",
};

const pushSnapshot = () => emit("snapshot", state);

// ------------------------------------------------------------------------------------------------ overlay

let overlay: OverlayState = { mode: { kind: "hidden" }, liveText: "" };
let levelTimer: number | undefined;

/** Dev harness hook (overlay.html in a browser): switch the pill mode. */
export function setMockOverlay(mode: OverlayMode, liveText = "") {
  overlay = { mode, liveText };
  emit("overlay", overlay);
  clearInterval(levelTimer);
  if (mode.kind === "listening") {
    let t = 0;
    levelTimer = window.setInterval(() => {
      t += 1;
      const v = Math.max(0, 0.08 + 0.07 * Math.sin(t / 2.3) + (Math.random() - 0.5) * 0.12);
      emit("level", v);
    }, 50);
  }
}

// ------------------------------------------------------------------------------------------------ meetings

let liveTimer: number | undefined;
const LIVE_LINES: [string, string][] = [
  ["You", "Okay, I think we're recording. Can everyone hear me?"],
  ["Them", "Yep, loud and clear."],
  ["You", "Great. First item is the pricing page, then the launch checklist."],
  ["Them", "On pricing, we still need the annual discount copy from marketing."],
  ["You", "I'll chase that today and share it in Slack."],
  ["Them", "Launch checklist is at eighty percent. QA signs off on Wednesday."],
  ["You", "Perfect. Let's keep Thursday as the go/no-go."],
];

function markdown(m: Meeting): string {
  const clock = (t: number) => {
    const s = Math.floor(t);
    const pad = (n: number) => String(n).padStart(2, "0");
    return s >= 3600 ? `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}` : `${Math.floor(s / 60)}:${pad(s % 60)}`;
  };
  const transcript = [...m.segments].sort((a, b) => a.time - b.time).map((s) => `[${clock(s.time)}] ${s.speaker}: ${s.text}`).join("\n");
  const start = new Date((m.start + APPLE_EPOCH_OFFSET) * 1000);
  let md = `# ${m.title}\n\n${start.toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" })} · ${Math.floor(((m.end ?? nowApple()) - m.start) / 60)} min`;
  if (m.app) md += ` · ${APPS[m.app] ?? m.app}`;
  if (m.summary) md += `\n\n${m.summary}`;
  if (m.todos.length) md += "\n\n## To-dos\n\n" + m.todos.map((t) => `- [${t.done ? "x" : " "}] ${t.text}`).join("\n");
  if (m.notes) md += `\n\n## My notes\n\n${m.notes}`;
  return md + `\n\n## Transcript\n\n${transcript}\n`;
}

function summarize(id: string) {
  const m = state.meetings.find((x) => x.id === id);
  if (!m || !m.segments.length || state.summarizing.includes(id)) return;
  state.summarizing = [...state.summarizing, id];
  pushSnapshot();
  setTimeout(() => {
    m.summary = `## Summary\n- ${m.segments.length} lines discussed.\n- ${m.segments[0].text}\n\n## Decisions\n- None\n\n## Action items\n- Them: follow up on the open points\n\n## Your to-dos\n- Share the notes with the team`;
    const done = new Map(m.todos.map((t) => [t.text, t.done]));
    m.todos = ["Share the notes with the team"].map((text) => ({ id: uuid(), text, done: done.get(text) ?? false }));
    if (m.titleAuto && m.end != null && m.title.startsWith("Meeting")) m.title = m.segments.length > 3 ? "Pricing page and launch checklist" : "Café launch check-in";
    state.summarizing = state.summarizing.filter((x) => x !== id);
    pushSnapshot();
  }, 2500);
}

function startMeeting(app?: string) {
  if (state.liveMeetingId) return;
  const m: Meeting = {
    id: uuid(),
    title: `${app ? (APPS[app] ?? app) + " call" : "Meeting"} · now`,
    titleAuto: true,
    todos: [],
    app,
    start: nowApple(),
    segments: [],
    summary: "",
    notes: "",
  };
  state.meetings = [m, ...state.meetings];
  state.liveMeetingId = m.id;
  setMockOverlay({ kind: "meetingLive", start: m.start });
  let i = 0;
  liveTimer = window.setInterval(() => {
    const [speaker, text] = LIVE_LINES[i % LIVE_LINES.length];
    m.segments.push(seg(nowApple() - m.start, speaker, text));
    i += 1;
    pushSnapshot();
  }, 2500);
  pushSnapshot();
}

function stopMeeting() {
  const m = state.meetings.find((x) => x.id === state.liveMeetingId);
  clearInterval(liveTimer);
  state.liveMeetingId = null;
  if (m) m.end = nowApple();
  setMockOverlay({ kind: "hidden" });
  pushSnapshot();
  if (m) summarize(m.id);
}

// ------------------------------------------------------------------------------------------------ downloads

const downloads = new Map<string, number>();

function download(file: string) {
  if (downloads.has(file)) return;
  let p = 0;
  const timer = window.setInterval(() => {
    p = Math.min(1, p + 0.04 + Math.random() * 0.04);
    const done = p >= 1;
    const ev: DownloadProgress = { file, progress: done ? 1 : p, done, error: null };
    if (done) {
      clearInterval(timer);
      downloads.delete(file);
      if (file.endsWith(".gguf")) state.installedModels = [...new Set([...state.installedModels, file])].sort();
      if (file === whisperCatalog.file) state.whisperModelInstalled = true;
      pushSnapshot();
    }
    emit("download", ev);
  }, 250);
  downloads.set(file, timer);
  emit("download", { file, progress: 0, done: false, error: null } satisfies DownloadProgress);
}

// ------------------------------------------------------------------------------------------------ commands

let launchAtLogin = false;

const devices: InputDevice[] = [
  { uid: "BuiltInMicrophoneDevice", name: "MacBook Pro Microphone", isBuiltIn: true, isContinuity: false },
  { uid: "AirPodsPro-UID", name: "Sam's AirPods Pro", isBuiltIn: false, isContinuity: false },
  { uid: "ShureMV7-UID", name: "Shure MV7", isBuiltIn: false, isContinuity: false },
];

const appInfo = (bundleId: string): AppInfo | null =>
  APPS[bundleId] ? { bundleId, name: APPS[bundleId], icon: iconFor(bundleId) } : null;

type Handler = (args: Record<string, any>) => unknown;

const handlers: Record<string, Handler> = {
  snapshot: () => state,
  set_config: ({ config }) => {
    state.config = config;
    pushSnapshot();
  },
  delete_dictation: ({ id }) => {
    state.history = state.history.filter((d) => d.id !== id);
    pushSnapshot();
  },
  clear_history: () => {
    state.history = [];
    pushSnapshot();
  },
  set_correction: ({ id, text }) => {
    const d = state.history.find((x) => x.id === id);
    if (!d) return;
    const t = String(text).trim();
    if (!t || t === d.text) delete d.correction;
    else d.correction = t;
    state.correctionCount = state.history.filter((x) => x.correction).length;
    pushSnapshot();
  },
  start_meeting: () => startMeeting(),
  stop_meeting: () => stopMeeting(),
  summarize_meeting: ({ id }) => summarize(id),
  update_meeting: ({ meeting }) => {
    const i = state.meetings.findIndex((m) => m.id === meeting.id);
    if (i < 0) return;
    const old = state.meetings[i];
    // Keep the live segment list authoritative; take the user's edits.
    state.meetings[i] = { ...old, title: meeting.title, titleAuto: meeting.title !== old.title ? false : old.titleAuto, notes: meeting.notes, todos: meeting.todos };
    pushSnapshot();
  },
  delete_meeting: ({ id }) => {
    state.meetings = state.meetings.filter((m) => m.id !== id);
    pushSnapshot();
  },
  meeting_markdown: ({ id }) => {
    const m = state.meetings.find((x) => x.id === id);
    return m ? markdown(m) : "";
  },
  write_text_file: ({ path, text }) => console.info(`[mock] write ${path} (${String(text).length} chars)`),
  download_model: ({ file }) => download(file),
  cancel_download: ({ file }) => {
    clearInterval(downloads.get(file));
    downloads.delete(file);
  },
  set_model: ({ file }) => {
    state.config = { ...state.config, model: file };
    state.polishReady = false;
    state.status = "Loading models…";
    pushSnapshot();
    setTimeout(() => {
      state.polishReady = true;
      state.status = "Ready — hold ⌘; to dictate";
      pushSnapshot();
    }, 1500);
  },
  set_asr_version: ({ version }) => {
    state.config = { ...state.config, asrVersion: version };
    pushSnapshot();
  },
  input_devices: () => devices,
  running_apps: () =>
    ["com.apple.Safari", "com.tinyspeck.slackmacgap", "com.apple.Notes", "com.linear", "us.zoom.xos", "com.himanshuat.openflow"].map((b) => appInfo(b)!),
  app_info: ({ bundleId }) => appInfo(bundleId),
  open_path: ({ which }) => console.info(`[mock] open ${which}`),
  copy_text: ({ text }) => navigator.clipboard?.writeText(text).catch(() => {}),
  get_launch_at_login: () => launchAtLogin,
  set_launch_at_login: ({ enabled }) => {
    launchAtLogin = enabled;
  },
  overlay_action: ({ action }) => {
    if (action === "startMeeting") startMeeting(overlay.mode.kind === "meetingOffer" ? "us.zoom.xos" : undefined);
    else if (action === "stopMeeting") stopMeeting();
    else setMockOverlay({ kind: "hidden" });
  },
  overlay_state: () => overlay,
};

export async function mockInvoke(cmd: string, args: Record<string, unknown>): Promise<unknown> {
  const h = handlers[cmd];
  if (!h) throw new Error(`[mock] unknown command ${cmd}`);
  await new Promise((r) => setTimeout(r, 10));
  const r = await h(args);
  return r === undefined ? null : clone(r);
}
