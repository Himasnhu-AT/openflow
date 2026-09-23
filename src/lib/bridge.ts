// Transport under `api.ts`. Inside Tauri this is exactly `@tauri-apps/api` invoke/listen and the dialog plugin.
// In a plain browser (`pnpm dev` without Tauri) it falls back to the in-memory mock in `mock.ts`, which is
// loaded lazily so it never ships in the Tauri code path.

import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen as tauriListen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Mock = typeof import("./mock");
let mockPromise: Promise<Mock> | null = null;
const mock = () => (mockPromise ??= import("./mock"));

export function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  return mock().then((m) => m.mockInvoke(cmd, args ?? {}) as Promise<T>);
}

export function listen<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  if (isTauri) return tauriListen<T>(event, handler);
  return mock().then((m) => m.mockListen<T>(event, handler));
}

/** Save panel. Returns the chosen path, or null when cancelled. */
export async function saveDialog(defaultPath: string): Promise<string | null> {
  if (!isTauri) return `/tmp/${defaultPath}`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath, filters: [{ name: "Markdown", extensions: ["md"] }] });
}
