// The floating pill (Overlay.swift). Rust sizes and positions the window (200×52, 360×52 for meetings, 520×74
// with live text); this just renders the capsule centered in it.
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { Mic, UsersRound, X } from "lucide-react";
import { api, events, fromAppleDate, type OverlayMode, type OverlayState } from "../lib/api";
import { isTauri } from "../lib/bridge";
import { clock } from "../lib/format";
import "./overlay.css";
import { forwardErrors } from "../lib/errors";

forwardErrors("overlay");

const BARS = 14;

/** Live text cut at the head so the newest words always show, up to 2 lines (truncationMode(.head)). */
function HeadTruncated({ text }: { text: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [shown, setShown] = useState(text);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const max = parseFloat(getComputedStyle(el).lineHeight) * 2 + 1;
    el.textContent = text;
    if (el.scrollHeight <= max) return setShown(text);
    // Smallest cut from the head that fits.
    let lo = 0;
    let hi = text.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      el.textContent = "…" + text.slice(mid);
      if (el.scrollHeight <= max) hi = mid;
      else lo = mid + 1;
    }
    setShown("…" + text.slice(lo));
  }, [text]);
  return (
    <div className="pill-live-wrap">
      <div ref={ref} className="pill-live measure" aria-hidden />
      <div className="pill-live">{shown}</div>
    </div>
  );
}

function Bars({ levels }: { levels: number[] }) {
  return (
    <div className="pill-bars">
      {levels.map((v, i) => (
        <div key={i} className="pill-bar" style={{ height: 4 + v * 20 }} />
      ))}
    </div>
  );
}

function Dots() {
  const [phase, setPhase] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setPhase((p) => (p + 1) % 3), 300);
    return () => clearInterval(t);
  }, []);
  return (
    <div style={{ display: "flex", gap: 5 }}>
      {[0, 1, 2].map((i) => (
        <span key={i} className="pill-dot" style={{ opacity: phase === i ? 1 : 0.3 }} />
      ))}
    </div>
  );
}

function Elapsed({ start }: { start: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);
  return <span className="pill-text tabular">Taking notes · {clock((now - fromAppleDate(start).getTime()) / 1000)}</span>;
}

function Pill({ state, levels }: { state: OverlayState; levels: number[] }) {
  const mode = state.mode;
  if (mode.kind === "hidden") return null;
  const showsLive = mode.kind === "listening" && !!state.liveText;
  const wide = mode.kind === "meetingOffer" || mode.kind === "meetingLive" || showsLive;

  let body: React.ReactNode;
  switch (mode.kind) {
    case "listening":
      body = (
        <>
          <Mic size={14} strokeWidth={2.6} color="var(--accent)" fill="var(--accent)" style={{ flex: "none" }} />
          <Bars levels={levels} />
          {showsLive && <HeadTruncated text={state.liveText} />}
        </>
      );
      break;
    case "processing":
      body = (
        <>
          <Dots />
          <span className="pill-text">Transcribing</span>
        </>
      );
      break;
    case "message":
      body = <span className="pill-text one-line">{mode.text}</span>;
      break;
    case "meetingOffer":
      body = (
        <>
          <UsersRound size={15} strokeWidth={2.4} color="var(--accent)" style={{ flex: "none" }} />
          <span className="pill-text one-line" style={{ flex: "0 1 auto" }}>
            Meeting in {mode.app}
          </span>
          <span style={{ flex: 1, minWidth: 4 }} />
          <button className="pill-button" onClick={() => api.overlayAction("startMeeting")}>
            Start taking notes
          </button>
          <button className="pill-close" aria-label="Dismiss" onClick={() => api.overlayAction("dismissMeeting")}>
            <X size={12} strokeWidth={3.2} />
          </button>
        </>
      );
      break;
    case "meetingLive":
      body = (
        <>
          <span className="pill-red" />
          <Elapsed start={mode.start} />
          <span style={{ flex: 1, minWidth: 4 }} />
          <button className="pill-button" onClick={() => api.overlayAction("stopMeeting")}>
            Stop
          </button>
        </>
      );
      break;
  }
  return <div className={"pill" + (wide ? " wide" : "")}>{body}</div>;
}

function Overlay() {
  const [state, setState] = useState<OverlayState>({ mode: { kind: "hidden" }, liveText: "" });
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const lastKind = useRef<OverlayMode["kind"]>("hidden");

  useEffect(() => {
    const apply = (s: OverlayState) => {
      // Levels are reset every time the pill starts listening.
      if (s.mode.kind === "listening" && lastKind.current !== "listening") setLevels(Array(BARS).fill(0));
      lastKind.current = s.mode.kind;
      setState(s);
    };
    const un1 = events.overlay(apply);
    const un2 = events.level((v) => setLevels((l) => [...l.slice(1), Math.min(1, v * 4)]));
    api.overlayState().then(apply).catch(() => {});
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, []);

  const pill = <Pill state={state} levels={levels} />;
  if (isTauri) return <div className="pill-window">{pill}</div>;
  return <DevHarness state={state}>{pill}</DevHarness>;
}

/** Browser-only: a frame the size of the real window, plus buttons that drive the mock. */
function DevHarness({ state, children }: { state: OverlayState; children: React.ReactNode }) {
  const m = state.mode;
  const live = m.kind === "listening" && !!state.liveText;
  const w = live ? 520 : m.kind === "meetingOffer" || m.kind === "meetingLive" ? 360 : 200;
  const h = live ? 74 : 52;
  const set = async (mode: OverlayMode, liveText = "") => (await import("../lib/mock")).setMockOverlay(mode, liveText);
  const now = Date.now() / 1000 - 978307200;
  const modes: [string, () => void][] = [
    ["listening", () => set({ kind: "listening" })],
    ["listening + live text", () => set({ kind: "listening" }, "so I was thinking we could move the launch to next Thursday because the QA team still needs a couple of days to finish the regression pass on the new onboarding flow and the pricing page")],
    ["processing", () => set({ kind: "processing" })],
    ["message", () => set({ kind: "message", text: "Cloud unavailable, used local models" })],
    ["meetingOffer", () => set({ kind: "meetingOffer", app: "Zoom" })],
    ["meetingLive", () => set({ kind: "meetingLive", start: now - 754 })],
    ["hidden", () => set({ kind: "hidden" })],
  ];
  return (
    <div className="harness">
      <div className="harness-buttons">
        {modes.map(([label, fn]) => (
          <button key={label} onClick={fn}>
            {label}
          </button>
        ))}
      </div>
      <div className="harness-frame" style={{ width: w, height: h }}>
        <div className="pill-window">{children}</div>
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Overlay />
  </React.StrictMode>,
);
