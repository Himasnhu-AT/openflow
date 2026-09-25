// The popup answers "is it connected?": whether OpenFlow on this Mac answers, and what the Meet tab in front sees.
const $ = (id) => document.getElementById(id);
const set = (id, text, cls) => {
  $(id).textContent = text;
  $(`${id}-dot`).className = `dot ${cls}`;
};

chrome.runtime.sendMessage({ type: "hello" }, (ok) => {
  if (ok) set("app", "Connected to OpenFlow", "ok");
  else set("app", "OpenFlow isn't answering. Is the app open?", "bad");
});

chrome.tabs.query({ active: true, currentWindow: true }, ([tab]) => {
  if (!tab) return set("meet", "No tab", "warn");
  chrome.tabs.sendMessage(tab.id, { type: "state" }, (s) => {
    if (chrome.runtime.lastError || !s) return set("meet", "Open a Google Meet tab to see names", "warn");
    if (!s.inCall) return set("meet", "Meet tab open, not in a call yet", "warn");
    set("meet", `In a call with ${s.participants.length} other${s.participants.length === 1 ? "" : "s"}`, "ok");
    const lines = [];
    lines.push(s.active ? `Speaking now: ${s.active}` : "Nobody speaking right now");
    if (s.participants.length) lines.push(`People: ${s.participants.join(", ")}`);
    if (s.self) lines.push(`You: ${s.self}`);
    lines.push(s.captions ? "Captions: on (best for names)" : "Captions: off. Turn on CC in Meet for better names");
    $("detail").innerHTML = lines.map((l) => `<div>${l.replace(/[<&]/g, (c) => (c === "<" ? "&lt;" : "&amp;"))}</div>`).join("");
  });
});
