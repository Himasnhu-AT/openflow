"use client";

// The hold-to-dictate demo: two giant ⌘ ; caps (or the real ⌘; chord) drive a mock Mac text field.
// All text is synthetic; it illustrates what the app does with your voice.

import { useCallback, useEffect, useRef, useState } from "react";
import { useMediaQuery } from "@/lib/useMediaQuery";

type SceneId = "messages" | "mail" | "terminal";
type Phase = "idle" | "listening" | "processing" | "done";

type Scene = {
  app: string;
  tone: string;
  mono: boolean;
  them: string;
  placeholder: string;
  spoken: string;
  final: string;
};

const SCENES: Record<SceneId, Scene> = {
  messages: {
    app: "Messages",
    tone: "supercasual",
    mono: false,
    them: "are we still on for tomorrow?",
    placeholder: "iMessage",
    spoken: "um so tomorrow is fine no wait thursday works see you then",
    final: "thursday works see you then",
  },
  mail: {
    app: "Mail",
    tone: "professional",
    mono: false,
    them: "Re: Q3 deck",
    placeholder: "Write your reply",
    spoken: "hi jordan uh thanks for the deck can we move the review to thursday at two best sam",
    final: "Hi Jordan,\n\nThanks for the deck. Can we move the review to Thursday at 2?\n\nBest,\nSam",
  },
  terminal: {
    app: "Terminal",
    tone: "verbatim",
    mono: true,
    them: "",
    placeholder: "~ %",
    spoken: "cargo test",
    final: "cargo test",
  },
};

const SCENE_KEYS: SceneId[] = ["messages", "mail", "terminal"];
const BARS = 9;

const prefersReducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

export default function Demo() {
  const [sceneId, setSceneId] = useState<SceneId>("messages");
  const [phase, setPhase] = useState<Phase>("idle");
  const [count, setCount] = useState(0);
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const reduceMotion = useMediaQuery("(prefers-reduced-motion: reduce)");
  const coarse = useMediaQuery("(pointer: coarse)");
  const [finishes, setFinishes] = useState(0); // re-keys the field so the swap highlight replays

  const scene = SCENES[sceneId];
  const words = scene.spoken.split(" ");

  // Mirrors `phase` for window listeners and timers, which outlive a render. Every transition sets both.
  const phaseRef = useRef<Phase>("idle");
  const touched = useRef(false);
  const autoplaying = useRef(false);
  const pointerStarted = useRef(false);
  const screenRef = useRef<HTMLDivElement>(null);

  const press = useCallback(() => {
    if (phaseRef.current === "listening" || phaseRef.current === "processing") return;
    phaseRef.current = "listening";
    setCount(prefersReducedMotion() ? words.length : 1);
    setPhase("listening");
  }, [words.length]);

  const release = useCallback(() => {
    if (phaseRef.current !== "listening") return;
    autoplaying.current = false;
    phaseRef.current = "processing";
    setCount(words.length); // released early: the rest of the sentence was still "said"
    setPhase("processing");
  }, [words.length]);

  // Esc, or a scene switch mid-demo: drop everything, like cancelling a dictation.
  const cancel = useCallback(() => {
    autoplaying.current = false;
    phaseRef.current = "idle";
    setCount(0);
    setPhase("idle");
  }, []);

  // Quick tap toggles, like the app: tap to start, tap again to stop.
  const toggle = useCallback(() => {
    touched.current = true;
    if (phaseRef.current === "listening") release();
    else press();
  }, [press, release]);

  // Words arrive one at a time while listening; autoplay lets go once the sentence is complete.
  useEffect(() => {
    if (phase !== "listening") return;
    if (count < words.length) {
      const t = setTimeout(() => setCount((c) => c + 1), 150 + Math.random() * 120);
      return () => clearTimeout(t);
    }
    if (autoplaying.current) {
      const t = setTimeout(() => {
        if (!touched.current) release();
      }, 450);
      return () => clearTimeout(t);
    }
  }, [phase, count, words.length, release]);

  // "Transcribing", then the polished text lands. Cancelling changes phase, which clears this timer.
  useEffect(() => {
    if (phase !== "processing") return;
    const t = setTimeout(
      () => {
        phaseRef.current = "done";
        setPhase("done");
        setFinishes((n) => n + 1);
      },
      reduceMotion ? 0 : 520,
    );
    return () => clearTimeout(t);
  }, [phase, reduceMotion]);

  // Waveform bars in the pill (levels are only rendered while listening).
  useEffect(() => {
    if (phase !== "listening" || reduceMotion) return;
    const t = setInterval(() => setLevels(Array.from({ length: BARS }, () => Math.random())), 90);
    return () => clearInterval(t);
  }, [phase, reduceMotion]);

  // The real chord: ⌘; (Ctrl+; elsewhere). Release either key to finish; Esc cancels.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.code === "Semicolon" || e.key === ";")) {
        e.preventDefault();
        if (e.repeat) return;
        touched.current = true;
        press();
      } else if (e.key === "Escape" && (phaseRef.current === "listening" || phaseRef.current === "processing")) {
        cancel();
      }
    };
    const up = (e: KeyboardEvent) => {
      if (
        phaseRef.current === "listening" &&
        (e.code === "Semicolon" || e.key === ";" || e.key === "Meta" || e.key === "Control")
      ) {
        release();
      }
    };
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", release);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", release);
    };
  }, [press, release, cancel]);

  // Play it once, when the demo is on screen, for visitors who haven't touched anything yet.
  useEffect(() => {
    const node = screenRef.current;
    if (!node) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const start = () => {
      if (touched.current) return;
      autoplaying.current = true;
      press();
      if (prefersReducedMotion()) release();
    };
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          io.disconnect();
          timer = setTimeout(start, prefersReducedMotion() ? 0 : 900);
        }
      },
      { threshold: 0.6 },
    );
    io.observe(node);
    return () => {
      io.disconnect();
      clearTimeout(timer);
    };
    // Runs once on mount; `press` and `release` only depend on the first scene's word count.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const chooseScene = (id: SceneId) => {
    touched.current = true;
    cancel();
    setSceneId(id);
  };

  const keyHandlers = {
    onPointerDown: (e: React.PointerEvent<HTMLButtonElement>) => {
      e.preventDefault();
      touched.current = true;
      pointerStarted.current = true;
      e.currentTarget.setPointerCapture?.(e.pointerId);
      press();
    },
    onPointerUp: release,
    onPointerCancel: release,
    onLostPointerCapture: release,
    // Clicks without a pointer press (VoiceOver, switch access) toggle.
    onClick: () => {
      if (pointerStarted.current) {
        pointerStarted.current = false;
        return;
      }
      toggle();
    },
    onKeyDown: (e: React.KeyboardEvent<HTMLButtonElement>) => {
      if ((e.key === " " || e.key === "Enter") && !e.repeat) {
        e.preventDefault();
        toggle();
      }
    },
  };

  const held = phase === "listening";
  const sentBubble = phase === "done" && sceneId === "messages";
  let field: React.ReactNode;
  if (phase === "listening" || phase === "processing") {
    field = (
      <>
        <span className="field__live">{words.slice(0, count).join(" ")}</span>
        <span className="field__caret" />
      </>
    );
  } else if (phase === "done" && !sentBubble) {
    field = scene.mono ? `~ % ${scene.final}` : scene.final;
  } else {
    field = <span className="field__placeholder">{scene.placeholder}</span>;
  }

  return (
    <section className="rig" aria-label="Interactive demo">
      <div className="screen-wrap">
        <div className={`screen${scene.mono ? " screen--mono" : ""}`} ref={screenRef}>
          <div className="screen__bar">
            <span className="dots" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            <span className="screen__app">{scene.app}</span>
            <span className="screen__tone">{scene.tone}</span>
          </div>
          <div className="screen__thread">
            {scene.them && <p className="bubble bubble--them">{scene.them}</p>}
            {sentBubble && <p className="bubble bubble--me">{scene.final}</p>}
          </div>
          <div className="screen__field">
            <p
              key={phase === "done" ? `done-${finishes}` : "field"}
              className={`field${phase === "done" ? " field--swap" : ""}`}
              aria-live="polite"
            >
              {field}
            </p>
          </div>
        </div>
        <div className={`pill${phase === "processing" ? " is-processing" : ""}`} hidden={!(held || phase === "processing")}>
          <span className="pill__bars" aria-hidden="true">
            {levels.map((v, i) => (
              <i key={i} style={held ? { transform: `scaleY(${0.2 + v * 0.8})` } : undefined} />
            ))}
          </span>
          <span className="pill__label">{phase === "processing" ? "Transcribing" : "Listening"}</span>
        </div>
      </div>

      <div className="rig__keys">
        <button
          className={`key key--accent key--giant${held ? " is-down" : ""}`}
          type="button"
          aria-label="Hold to dictate (⌘)"
          aria-pressed={held}
          {...keyHandlers}
        >
          <span className="cap">
            <span className="glyph">⌘</span>
            <span className="sublegend">command</span>
          </span>
        </button>
        <button
          className={`key key--accent key--giant${held ? " is-down" : ""}`}
          type="button"
          aria-label="Hold to dictate (;)"
          aria-pressed={held}
          {...keyHandlers}
        >
          <span className="cap">
            <span className="glyph">;</span>
            <span className="sublegend">semicolon</span>
          </span>
        </button>
      </div>
      <p className="rig__hint">
        {coarse ? (
          "Press and hold the keys."
        ) : (
          <>
            Hold the keys. Or press <kbd>⌘</kbd>
            <kbd>;</kbd> on your keyboard.
          </>
        )}
      </p>

      <div className="rig__apps" role="group" aria-label="Try it in another app">
        {SCENE_KEYS.map((id) => (
          <button
            key={id}
            className={`key key--alpha key--small${sceneId === id ? " is-on" : ""}`}
            type="button"
            aria-pressed={sceneId === id}
            onClick={() => chooseScene(id)}
          >
            <span className="cap">{SCENES[id].app}</span>
          </button>
        ))}
      </div>
      <p className="note note--plate">Simulated demo. The app does this with your voice, in any app.</p>
    </section>
  );
}
