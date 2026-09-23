// Forwards uncaught frontend errors to the Rust log (they are otherwise invisible in the packaged app).
import { invoke, isTauri } from "./bridge";

export function forwardErrors(window_: string) {
  if (!isTauri) return;
  const send = (message: string) => void invoke("log_frontend", { window: window_, message }).catch(() => {});
  window.addEventListener("error", (e) => send(`${e.message} at ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => send(`unhandled rejection: ${String(e.reason)}`));
}
