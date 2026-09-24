// Ollama server status for the Models and Settings pages. `api.ollamaStatus()` queries the server, so it is
// fetched on mount, every few seconds while the window is visible, and after a pull finishes.

import { useCallback, useEffect, useState } from "react";
import { api, events, type OllamaStatus } from "./api";

export function useOllamaStatus(pollMs = 5000): { status: OllamaStatus | null; error: string | null; refresh: () => void } {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  /** The status command itself failed (not "Ollama is down", which is a normal status). */
  const [error, setError] = useState<string | null>(null);
  const refresh = useCallback(() => {
    api
      .ollamaStatus()
      .then((s) => {
        setStatus(s);
        setError(null);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    refresh();
    const tick = () => {
      if (document.visibilityState === "visible") refresh();
    };
    const timer = window.setInterval(tick, pollMs);
    document.addEventListener("visibilitychange", tick);
    // A finished pull changes the installed list (GGUF downloads end in .gguf / .bin and don't).
    const un = events.download((p) => {
      if (p.done && !/\.(gguf|bin)$/.test(p.file)) refresh();
    });
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
      un.then((f) => f());
    };
  }, [pollMs, refresh]);

  return { status, error, refresh };
}

/** Bytes as "3.4 GB". */
export const formatGB = (bytes: number) => `${(bytes / 1e9).toFixed(1)} GB`;
