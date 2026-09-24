// OpenFlow website: the hold-to-dictate demo, tone switcher and screenshot gallery.
// All demo text is synthetic; it illustrates what the app does with your voice.

// Replace these two with the real URLs before publishing.
const LINKS = {
  download: "https://github.com/REPLACE-ME/openflow/releases/latest",
  source: "https://github.com/REPLACE-ME/openflow",
};
for (const a of document.querySelectorAll("[data-link]")) {
  const href = LINKS[a.dataset.link];
  if (href) a.href = href;
}

const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ---------------------------------------------------------------- demo scenes

const SCENES = {
  messages: {
    app: "Messages",
    tone: "supercasual",
    mono: false,
    them: "are we still on for tomorrow?",
    placeholder: "iMessage",
    spoken: "um so tomorrow is fine no wait thursday works see you then",
    final: "thursday works, see you then",
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
    spoken: "git rebase dash i head tilde three",
    final: "git rebase -i HEAD~3",
  },
};

const el = {
  screen: document.getElementById("screen"),
  app: document.getElementById("screen-app"),
  tone: document.getElementById("screen-tone"),
  them: document.getElementById("bubble-them"),
  thread: document.getElementById("screen-thread"),
  field: document.getElementById("field"),
  pill: document.getElementById("pill"),
  pillLabel: document.getElementById("pill-label"),
  bars: [...document.querySelectorAll(".pill__bars i")],
  keys: [document.getElementById("key-cmd"), document.getElementById("key-semi")],
  hint: document.getElementById("rig-hint"),
};

let scene = SCENES.messages;
let state = "idle"; // idle | listening | processing | done
let words = [];
let spokenCount = 0;
let wordTimer = null;
let barTimer = null;
let touched = false;

function renderScene() {
  el.app.textContent = scene.app;
  el.tone.textContent = scene.tone;
  el.screen.classList.toggle("screen--mono", scene.mono);
  el.thread.querySelectorAll(".bubble--me").forEach((b) => b.remove());
  el.them.textContent = scene.them;
  el.them.hidden = !scene.them;
  showPlaceholder();
}

function showPlaceholder() {
  el.field.replaceChildren(span("field__placeholder", scene.placeholder));
}

function span(cls, text) {
  const s = document.createElement("span");
  s.className = cls;
  s.textContent = text;
  return s;
}

function showLive(text) {
  const caret = span("field__caret", "");
  el.field.replaceChildren(span("field__live", text), caret);
}

function setKeys(down) {
  for (const k of el.keys) {
    k.classList.toggle("is-down", down);
    k.setAttribute("aria-pressed", String(down));
  }
}

function startBars() {
  stopBars();
  if (reduceMotion) return;
  barTimer = setInterval(() => {
    for (const b of el.bars) b.style.transform = `scaleY(${0.2 + Math.random() * 0.8})`;
  }, 90);
}
function stopBars() {
  clearInterval(barTimer);
  for (const b of el.bars) b.style.transform = "";
}

function press() {
  if (state === "listening" || state === "processing") return;
  if (state === "done") renderScene();
  state = "listening";
  words = scene.spoken.split(" ");
  spokenCount = 0;
  setKeys(true);
  el.pill.hidden = false;
  el.pill.classList.remove("is-processing");
  el.pillLabel.textContent = "Listening";
  startBars();
  if (reduceMotion) {
    spokenCount = words.length;
    showLive(scene.spoken);
    return;
  }
  const next = () => {
    if (state !== "listening") return;
    if (spokenCount < words.length) {
      spokenCount += 1;
      showLive(words.slice(0, spokenCount).join(" "));
      wordTimer = setTimeout(next, 150 + Math.random() * 120);
    }
  };
  next();
}

function release() {
  if (state !== "listening") return;
  clearTimeout(wordTimer);
  setKeys(false);
  // Released early: the rest of the sentence was still "said".
  showLive(scene.spoken);
  state = "processing";
  stopBars();
  el.pill.classList.add("is-processing");
  el.pillLabel.textContent = "Transcribing";
  setTimeout(finish, reduceMotion ? 0 : 520);
}

function finish() {
  state = "done";
  el.pill.hidden = true;
  if (scene.mono) {
    el.field.replaceChildren(document.createTextNode(`~ % ${scene.final}`));
  } else if (scene.app === "Messages") {
    const sent = document.createElement("p");
    sent.className = "bubble bubble--me";
    sent.textContent = scene.final;
    el.thread.append(sent);
    showPlaceholder();
  } else {
    el.field.replaceChildren(document.createTextNode(scene.final));
  }
  el.field.classList.remove("field--swap");
  void el.field.offsetWidth;
  el.field.classList.add("field--swap");
}

// Pointer: holding either on-screen key holds the chord.
for (const k of el.keys) {
  k.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    touched = true;
    k.setPointerCapture?.(e.pointerId);
    press();
  });
  k.addEventListener("pointerup", release);
  k.addEventListener("pointercancel", release);
  k.addEventListener("lostpointercapture", release);
  // Keyboard access to the on-screen keys: Space/Enter held.
  k.addEventListener("keydown", (e) => {
    if ((e.key === " " || e.key === "Enter") && !e.repeat) {
      e.preventDefault();
      touched = true;
      press();
    }
  });
  k.addEventListener("keyup", (e) => {
    if (e.key === " " || e.key === "Enter") release();
  });
}

// The real chord: ⌘; (Ctrl+; elsewhere). Release either key to finish.
window.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && (e.code === "Semicolon" || e.key === ";")) {
    e.preventDefault();
    if (e.repeat) return;
    touched = true;
    press();
  } else if (e.key === "Escape" && state === "listening") {
    clearTimeout(wordTimer);
    setKeys(false);
    stopBars();
    el.pill.hidden = true;
    state = "idle";
    showPlaceholder();
  }
});
window.addEventListener("keyup", (e) => {
  if (state === "listening" && (e.code === "Semicolon" || e.key === ";" || e.key === "Meta" || e.key === "Control")) {
    release();
  }
});
window.addEventListener("blur", release);

// Scenes.
const sceneKeys = [...document.querySelectorAll("[data-scene]")];
for (const b of sceneKeys) {
  b.addEventListener("click", () => {
    if (state === "listening" || state === "processing") return;
    touched = true;
    for (const o of sceneKeys) {
      o.classList.toggle("is-on", o === b);
      o.setAttribute("aria-pressed", String(o === b));
    }
    scene = SCENES[b.dataset.scene];
    state = "idle";
    renderScene();
  });
}

renderScene();

// Play it once for visitors who haven't touched anything yet, so the first viewport shows the mechanism.
if (!reduceMotion) {
  setTimeout(() => {
    if (touched) return;
    press();
    setTimeout(() => {
      if (!touched) release();
    }, 150 * scene.spoken.split(" ").length + 700);
  }, 1400);
} else {
  press();
  release();
}

// ---------------------------------------------------------------- tones

const TONES = {
  messages: { name: "supercasual · Messages", text: "hey Jordan thanks for the deck can we move the review to thursday?", mono: false },
  mail: { name: "professional · Mail", text: "Hey Jordan, thanks for the deck. Can we move the review to Thursday?", mono: false },
  notes: { name: "casual · Notes", text: "hey Jordan, thanks for the deck. can we move the review to thursday?", mono: false },
  terminal: { name: "verbatim · Terminal", text: "hey jordan um thanks for the deck can we move the review to thursday", mono: true },
};
const toneKeys = [...document.querySelectorAll("[data-tone]")];
const toneName = document.getElementById("tone-name");
const toneText = document.getElementById("tone-text");
function setTone(id) {
  const t = TONES[id];
  toneName.textContent = t.name;
  toneText.textContent = t.text;
  toneText.parentElement.classList.toggle("is-mono", t.mono);
  for (const k of toneKeys) {
    const on = k.dataset.tone === id;
    k.classList.toggle("is-on", on);
    k.setAttribute("aria-selected", String(on));
  }
}
for (const k of toneKeys) k.addEventListener("click", () => setTone(k.dataset.tone));
setTone("messages");

// ---------------------------------------------------------------- gallery

const SHOTS = {
  home: { src: "assets/app-home.jpg", w: 1540, h: 1250, alt: "OpenFlow's history of recent dictations, each with the app and tone it was written in." },
  style: { src: "assets/app-style.jpg", w: 1600, h: 926, alt: "OpenFlow's Style page: editable tones (casual, professional, supercasual, verbatim) with the instruction for each." },
  insights: { src: "assets/app-insights.jpg", w: 1600, h: 926, alt: "OpenFlow's Insights page: words per minute, total words, streak, and usage by app and tone." },
};
const shotKeys = [...document.querySelectorAll("[data-shot]")];
const shot = document.getElementById("shot");
for (const k of shotKeys) {
  k.addEventListener("click", () => {
    const s = SHOTS[k.dataset.shot];
    shot.src = s.src;
    shot.width = s.w;
    shot.height = s.h;
    shot.alt = s.alt;
    for (const o of shotKeys) {
      o.classList.toggle("is-on", o === k);
      o.setAttribute("aria-selected", String(o === k));
    }
  });
}
