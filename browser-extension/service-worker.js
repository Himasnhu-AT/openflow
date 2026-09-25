// Forwards the Meet tab's observations to the OpenFlow app on this Mac. Content scripts can't reach 127.0.0.1
// themselves (they fetch as the page); the service worker can, and its requests carry this extension's Origin,
// which is the only one the app accepts. If the app isn't running the request fails and is dropped.
const ENDPOINT = "http://127.0.0.1:8770/meet";
const TYPES = new Set(["speaker", "caption", "call", "hello"]);

/** Posts one message to the app. Resolves to true when OpenFlow answered. */
function post(msg) {
  return fetch(ENDPOINT, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(msg),
  })
    .then((r) => r.ok)
    .catch(() => false);
}

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (!msg || !TYPES.has(msg.type)) return;
  // From a Meet tab: fire and forget. From the popup (no tab): answer whether OpenFlow is reachable.
  if (sender.tab) {
    post(msg);
    return;
  }
  post(msg).then(sendResponse);
  return true;
});
