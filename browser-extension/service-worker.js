// Forwards the Meet tab's observations to the OpenFlow app on this Mac. Content scripts can't reach 127.0.0.1
// themselves (they fetch as the page); the service worker can, and its requests carry this extension's Origin,
// which is the only one the app accepts. If the app isn't running the request fails and is dropped.
const ENDPOINT = "http://127.0.0.1:8770/meet";
const TYPES = new Set(["speaker", "caption", "call"]);

chrome.runtime.onMessage.addListener((msg, sender) => {
  if (!sender.tab || !msg || !TYPES.has(msg.type)) return;
  fetch(ENDPOINT, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(msg),
  }).catch(() => {});
});
