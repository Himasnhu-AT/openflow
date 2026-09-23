// IPC contract between the React UI and the Rust app. Mirrors src-tauri/src/commands.rs and the DTOs in
// crates/openflow-core/src/{config,model}.rs (serde camelCase). Change both sides together.
//
// Dates: `number` = seconds since 2001-01-01 (Apple reference date), exactly as stored on disk.
// Use `fromAppleDate` to get a JS Date.

import type { UnlistenFn } from "@tauri-apps/api/event";
// Same invoke/listen as @tauri-apps/api inside Tauri; falls back to the in-memory mock in a plain browser.
import { invoke, listen } from "./bridge";

export const APPLE_EPOCH_OFFSET = 978307200;
export const fromAppleDate = (s: number) => new Date((s + APPLE_EPOCH_OFFSET) * 1000);

// ------------------------------------------------------------------------------------------------ DTOs

export interface ToneConfig {
  tone: string;
  alt: string;
}

export interface Config {
  asrVersion: "v2" | "v3" | string;
  llamaServerPath: string;
  model: string;
  llamaPort: number;
  microphone: string;
  hotkeyKeyCode: number;
  tapThreshold: number;
  defaultTones: ToneConfig;
  appTones: Record<string, ToneConfig>;
  tones: Record<string, string>;
  dictionary: string[];
  learnedSignals: string[];
  memory: string;
  keepClips: boolean;
  overlay: boolean;
  sounds: boolean;
  fieldContext: boolean;
  liveText: boolean;
  meetingDetect: boolean;
  meetingEngine: "auto" | "english" | string;
  chineseScript: "traditional" | "simplified" | string;
  useContacts: boolean;
  windowAtLaunch: boolean;
  cloudEnabled: boolean;
  cloudDictation: "off" | "long" | "all" | string;
  cloudMeetings: boolean;
  liveTextExcluded: string[];
  cloudAPIKey: string;
  cloudASRModel: string;
  cloudLLMModel: string;
  whisperServerPath: string;
  whisperModel: string;
  meetingFarEngine: "whisper" | "parakeet" | string;
  notesModel: string;
}

export interface Dictation {
  id: string;
  date: number;
  text: string;
  raw: string;
  app?: string;
  tone: string;
  seconds: number;
  clip?: string;
  correction?: string;
}

export interface Stats {
  words: number;
  seconds: number;
  dictations: number;
  activeDays: string[];
  perApp: Record<string, number>;
  perTone: Record<string, number>;
}

export interface MeetingSegment {
  id: string;
  time: number;
  speaker: "You" | "Them" | string;
  text: string;
}

export interface MeetingTodo {
  id: string;
  text: string;
  done: boolean;
}

export interface Meeting {
  id: string;
  title: string;
  titleAuto: boolean;
  todos: MeetingTodo[];
  app?: string;
  start: number;
  end?: number;
  segments: MeetingSegment[];
  summary: string;
  notes: string;
}

export interface InputDevice {
  uid: string;
  name: string;
  isBuiltIn: boolean;
  isContinuity: boolean;
}

export interface AppInfo {
  bundleId: string;
  name: string;
  /** data: URL (PNG) */
  icon?: string;
}

export interface CatalogModel {
  file: string;
  label: string;
  url: string;
  gb: number;
  note: string;
}

/** Everything the main window renders. Emitted as the `snapshot` event on every change. */
export interface Snapshot {
  config: Config;
  history: Dictation[];
  stats: Stats;
  /** Computed by Rust (Stats.wordsPerMinute / streak). */
  wpm: number;
  streak: number;
  meetings: Meeting[];
  liveMeetingId: string | null;
  summarizing: string[];
  /** Status line, same text as the tray menu ("Ready — hold ⌘; to dictate"). */
  status: string;
  speechReady: boolean;
  polishReady: boolean;
  correctionCount: number;
  /** Clip file exists, per dictation id. */
  clipsPresent: string[];
  installedModels: string[];
  catalog: CatalogModel[];
  whisperCatalog: CatalogModel;
  whisperServerFound: boolean;
  /** `whisperCatalog.file` exists in models/ (it is a .bin, so it is not in installedModels). Optional: when
   *  absent the UI falls back to `installedModels.includes(whisperCatalog.file)`. */
  whisperModelInstalled?: boolean;
  dataDir: string;
}

export interface DownloadProgress {
  file: string;
  /** 0..1, null when the size is unknown */
  progress: number | null;
  done: boolean;
  error: string | null;
}

/** State of the floating pill (overlay window). */
export type OverlayMode =
  | { kind: "hidden" }
  | { kind: "listening" }
  | { kind: "processing" }
  | { kind: "message"; text: string }
  | { kind: "meetingOffer"; app: string }
  | { kind: "meetingLive"; start: number };

export interface OverlayState {
  mode: OverlayMode;
  /** Live transcript shown in the pill (only while listening, when it can't go into the field). */
  liveText: string;
}

// ------------------------------------------------------------------------------------------------ commands

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  /** Replace the whole config; saved immediately. Rust restarts engines when model/asrVersion changed. */
  setConfig: (config: Config) => invoke<void>("set_config", { config }),

  deleteDictation: (id: string) => invoke<void>("delete_dictation", { id }),
  clearHistory: () => invoke<void>("clear_history"),
  setCorrection: (id: string, text: string) => invoke<void>("set_correction", { id, text }),

  startMeeting: () => invoke<void>("start_meeting"),
  stopMeeting: () => invoke<void>("stop_meeting"),
  summarizeMeeting: (id: string) => invoke<void>("summarize_meeting", { id }),
  /** Title edits (sets titleAuto=false on the Rust side when the title changed), notes, todo toggles. */
  updateMeeting: (meeting: Meeting) => invoke<void>("update_meeting", { meeting }),
  deleteMeeting: (id: string) => invoke<void>("delete_meeting", { id }),
  meetingMarkdown: (id: string) => invoke<string>("meeting_markdown", { id }),
  /** Writes text to a path chosen with the dialog plugin. */
  writeTextFile: (path: string, text: string) => invoke<void>("write_text_file", { path, text }),

  downloadModel: (file: string, url: string) => invoke<void>("download_model", { file, url }),
  cancelDownload: (file: string) => invoke<void>("cancel_download", { file }),
  setModel: (file: string) => invoke<void>("set_model", { file }),
  setAsrVersion: (version: string) => invoke<void>("set_asr_version", { version }),

  inputDevices: () => invoke<InputDevice[]>("input_devices"),
  runningApps: () => invoke<AppInfo[]>("running_apps"),
  appInfo: (bundleId: string) => invoke<AppInfo | null>("app_info", { bundleId }),

  /** "config" | "models" | "readme" */
  openPath: (which: "config" | "models" | "readme") => invoke<void>("open_path", { which }),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  getLaunchAtLogin: () => invoke<boolean>("get_launch_at_login"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),

  /** Pill buttons. */
  overlayAction: (action: "startMeeting" | "dismissMeeting" | "stopMeeting") =>
    invoke<void>("overlay_action", { action }),
  overlayState: () => invoke<OverlayState>("overlay_state"),
  /** Page requested with `--settings <Page>` at launch; returned once, then null. */
  initialPage: () => invoke<string | null>("initial_page"),
};

// ------------------------------------------------------------------------------------------------ events

export const events = {
  snapshot: (cb: (s: Snapshot) => void): Promise<UnlistenFn> => listen<Snapshot>("snapshot", (e) => cb(e.payload)),
  overlay: (cb: (s: OverlayState) => void): Promise<UnlistenFn> =>
    listen<OverlayState>("overlay", (e) => cb(e.payload)),
  /** Mic RMS level while listening, ~20/s. */
  level: (cb: (v: number) => void): Promise<UnlistenFn> => listen<number>("level", (e) => cb(e.payload)),
  download: (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
    listen<DownloadProgress>("download", (e) => cb(e.payload)),
  /** Main window navigation requested by Rust (`--settings Models`, tray "Open OpenFlow…"). */
  navigate: (cb: (page: string) => void): Promise<UnlistenFn> => listen<string>("navigate", (e) => cb(e.payload)),
};
