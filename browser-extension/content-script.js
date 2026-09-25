// OpenFlow for Google Meet: who is speaking, and Meet's own captions when they are on.
//
// Meet's class names are obfuscated and change often, so nothing here depends on them:
//  - Participants are the elements carrying `data-participant-id` (stable for years).
//  - A tile's name is the short line of visible text in it that reads like a name.
//  - Speaking is detected from activity, not a class: while someone talks, Meet animates the audio indicator in
//    their tile, which shows up as a burst of attribute mutations inside it. A silent tile is static (video
//    frames are not DOM changes). The busiest non-self tile over the last 800 ms is the active speaker.
//  - Captions (when the user turned them on) are the region whose label mentions captions; each block reads as
//    "Name" then what they said.
// Only names and caption text leave the page, and only to the OpenFlow app on this Mac (see service-worker.js).

const UI_WORDS = new Set([
  "you", "presentation", "presenting", "is", "more", "options", "pin", "unpin", "mute", "muted", "unmute",
  "microphone", "mic", "camera", "turn", "meeting", "details", "chat", "people", "everyone", "raise", "hand",
  "captions", "settings", "backgrounds", "effects", "leave", "call", "joined", "left", "host", "controls", "remove",
  "screen", "share", "sharing", "reactions", "others", "waiting", "admit", "view", "tile", "spotlight", "frame",
  "videocam", "keep", "visibility", "close", "fullscreen", "person", "hand", "expand", "collapse",
]);

function validName(s) {
  const n = (s || "").trim();
  // Also not a web address, and not a Material icon ligature ("mic_off", "more_vert") read as text.
  if (n.length < 2 || n.length > 40 || !/\p{L}/u.test(n) || /[:/_@]/.test(n) || /www\.|\.(com|org|net)\b/i.test(n)) return false;
  const words = n.split(/\s+/);
  if (words.length > 5) return false;
  return !words.some((w) => UI_WORDS.has(w.toLowerCase().replace(/[^\p{L}]/gu, "")));
}

/** "Priya Shah (You)" -> "Priya Shah" */
function canon(s) {
  return (s || "").replace(/\s*\((you|me|host|presenting)[^)]*\)\s*$/i, "").trim();
}

/** Name and self-ness from a tile's visible text lines. */
function nameFromLines(lines) {
  const clean = lines.map((l) => l.trim()).filter(Boolean);
  const isSelf = clean.some((l) => /^you$/i.test(l) || /\((you|me)\)$/i.test(l));
  const name = clean.map(canon).find(validName) || null;
  return { name, isSelf };
}

/** A caption block's text lines -> {name, text}, or null. The name comes first, then what they said. */
function parseCaption(lines) {
  const clean = lines.map((l) => l.trim()).filter(Boolean);
  if (clean.length < 2) return null;
  const name = canon(clean[0]);
  if (!validName(name) && !/^you$/i.test(name)) return null;
  return { name, text: clean.slice(1).join(" ") };
}

/** The active speaker: the non-self participant with the most recent activity, if clearly speaking. */
function pickActive(activity, selfIds, minEvents) {
  let best = null;
  let bestCount = 0;
  for (const [id, count] of activity) {
    if (selfIds.has(id) || count < minEvents) continue;
    if (count > bestCount) {
      best = id;
      bestCount = count;
    }
  }
  return best;
}

if (typeof module !== "undefined") {
  module.exports = { validName, canon, nameFromLines, parseCaption, pickActive };
}

if (typeof document !== "undefined" && typeof chrome !== "undefined" && chrome.runtime) {
  const WINDOW_MS = 800;
  const MIN_EVENTS = 3;
  /** participant id -> timestamps of recent mutations inside its tile */
  const events = new Map();
  let last = { active: undefined, participants: "" };
  let lastSent = 0;
  let inCall = false;
  const captionSeen = new Map();
  /** What this tab sees, for the extension's popup. */
  const state = { inCall: false, active: null, self: null, participants: [], captions: false };

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (msg && msg.type === "state") sendResponse(state);
  });

  const send = (msg) => {
    try {
      // The service worker never answers; without the catch every send logs "message port closed".
      chrome.runtime.sendMessage(msg).catch(() => {});
    } catch (_) {
      // The extension was reloaded; this page's script is orphaned until the tab reloads.
    }
  };

  // Check in every 10 s so OpenFlow can show the extension as connected, in a call or not.
  send({ type: "hello" });
  setInterval(() => send({ type: "hello" }), 10_000);

  const tileOf = (el) => (el && el.closest ? el.closest("[data-participant-id]") : null);

  new MutationObserver((records) => {
    const now = performance.now();
    for (const r of records) {
      const t = r.target;
      if (!t || t.nodeType !== 1 || t.tagName === "VIDEO" || t.tagName === "CANVAS") continue;
      const tile = tileOf(t);
      if (!tile) continue;
      const id = tile.getAttribute("data-participant-id");
      const list = events.get(id) || [];
      list.push(now);
      events.set(id, list);
    }
  }).observe(document.documentElement, { subtree: true, attributes: true, attributeFilter: ["class", "style"] });

  function participants() {
    const byId = new Map();
    for (const el of document.querySelectorAll("[data-participant-id]")) {
      const id = el.getAttribute("data-participant-id");
      const selfAttr = el.querySelector("[data-self-name]");
      const { name, isSelf } = nameFromLines((el.innerText || "").split("\n"));
      const prev = byId.get(id) || { name: null, isSelf: false };
      byId.set(id, {
        name: prev.name || (selfAttr && canon(selfAttr.getAttribute("data-self-name"))) || name,
        isSelf: prev.isSelf || isSelf || !!selfAttr,
      });
    }
    return byId;
  }

  function tick() {
    const people = participants();
    const nowInCall = people.size > 0;
    if (nowInCall !== inCall) {
      inCall = nowInCall;
      state.inCall = inCall;
      send({ type: "call", inCall });
    }
    if (!inCall) return;

    const now = performance.now();
    const counts = new Map();
    for (const [id, list] of events) {
      const recent = list.filter((t) => now - t < WINDOW_MS);
      if (recent.length) events.set(id, recent);
      else events.delete(id);
      counts.set(id, recent.length);
    }
    const selfIds = new Set([...people].filter(([, p]) => p.isSelf).map(([id]) => id));
    const activeId = pickActive(counts, selfIds, MIN_EVENTS);
    const active = activeId ? (people.get(activeId) || {}).name || null : null;
    const self = [...people.values()].find((p) => p.isSelf && p.name)?.name || null;
    const names = [...new Set([...people.values()].filter((p) => p.name && !p.isSelf).map((p) => p.name))];
    const key = names.join("|");
    Object.assign(state, { active, self, participants: names });
    // On change, and every 3 s while someone talks so the app knows they still are.
    if (active !== last.active || key !== last.participants || (active && now - lastSent > 3000)) {
      last = { active, participants: key };
      lastSent = now;
      send({ type: "speaker", active, self, participants: names });
    }
    readCaptions();
  }

  function readCaptions() {
    const region = [...document.querySelectorAll('[role="region"][aria-label]')].find((r) =>
      /caption|subtit|untertitel|sous-titres|legenda|字幕/i.test(r.getAttribute("aria-label")),
    );
    if (!region) return;
    // Blocks are the region's children that hold both a name and text; take the last three (the live ones).
    const blocks = [...region.querySelectorAll(":scope > div, :scope > div > div")]
      .map((b) => parseCaption((b.innerText || "").split("\n")))
      .filter(Boolean)
      .slice(-3);
    for (const c of blocks) {
      if (/^you$/i.test(c.name)) continue;
      const k = c.name;
      if (captionSeen.get(k) === c.text) continue;
      captionSeen.set(k, c.text);
      state.captions = true;
      send({ type: "caption", name: c.name, text: c.text.slice(0, 2000) });
    }
  }

  setInterval(tick, 400);
}
