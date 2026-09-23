// The main window's single source of state: `api.snapshot()` once, then every `snapshot` event.
// Config and meeting edits are applied locally first (optimistic), then sent to Rust; text edits are debounced.

import { createContext, useContext, useEffect, useMemo, useRef, useState } from "react";
import { api, events, type Config, type Meeting, type Snapshot } from "./api";

const DEBOUNCE_MS = 300;

export interface Store {
  snap: Snapshot;
  /** Apply a config change. `debounce` for text fields (~300 ms); toggles/pickers go out immediately. */
  setConfig: (change: Partial<Config> | ((c: Config) => Config), opts?: { debounce?: boolean }) => void;
  /** Apply a meeting edit (title, notes, todos). */
  updateMeeting: (m: Meeting, opts?: { debounce?: boolean }) => void;
}

export function useSnapshotStore(): Store | null {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const configRef = useRef<Config | null>(null);
  const configTimer = useRef<number | undefined>(undefined);
  const meetingTimers = useRef(new Map<string, number>());
  const pendingMeetings = useRef(new Map<string, Meeting>());

  useEffect(() => {
    let live = true;
    const apply = (s: Snapshot) => {
      if (!live) return;
      // While an edit is still waiting to go out, keep the local version so typing is never clobbered.
      if (configTimer.current !== undefined && configRef.current) s = { ...s, config: configRef.current };
      else configRef.current = s.config;
      if (pendingMeetings.current.size)
        s = { ...s, meetings: s.meetings.map((m) => (pendingMeetings.current.has(m.id) ? { ...pendingMeetings.current.get(m.id)!, segments: m.segments, summary: m.summary, end: m.end } : m)) };
      setSnap(s);
    };
    const un = events.snapshot(apply);
    api.snapshot().then(apply).catch((e) => console.error("snapshot failed", e));
    return () => {
      live = false;
      un.then((f) => f());
    };
  }, []);

  return useMemo(() => {
    if (!snap) return null;
    const setConfig: Store["setConfig"] = (change, opts) => {
      const base = configRef.current ?? snap.config;
      const next = typeof change === "function" ? change(base) : { ...base, ...change };
      configRef.current = next;
      setSnap((s) => (s ? { ...s, config: next } : s));
      window.clearTimeout(configTimer.current);
      const send = () => {
        configTimer.current = undefined;
        api.setConfig(configRef.current!).catch((e) => console.error("set_config failed", e));
      };
      if (opts?.debounce) configTimer.current = window.setTimeout(send, DEBOUNCE_MS);
      else send();
    };
    const updateMeeting: Store["updateMeeting"] = (m, opts) => {
      setSnap((s) => (s ? { ...s, meetings: s.meetings.map((x) => (x.id === m.id ? m : x)) } : s));
      const timers = meetingTimers.current;
      window.clearTimeout(timers.get(m.id));
      pendingMeetings.current.set(m.id, m);
      const send = () => {
        timers.delete(m.id);
        pendingMeetings.current.delete(m.id);
        api.updateMeeting(m).catch((e) => console.error("update_meeting failed", e));
      };
      if (opts?.debounce) timers.set(m.id, window.setTimeout(send, DEBOUNCE_MS));
      else send();
    };
    return { snap, setConfig, updateMeeting };
  }, [snap]);
}

export const StoreContext = createContext<Store | null>(null);

export function useStore(): Store {
  const s = useContext(StoreContext);
  if (!s) throw new Error("useStore outside StoreContext");
  return s;
}
