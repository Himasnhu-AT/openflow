import { useEffect, useState } from "react";
import { api, fromAppleDate, APPLE_EPOCH_OFFSET, type AppInfo, type Meeting } from "./api";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/** "h:mm a" */
export function timeHMA(d: Date): string {
  const h = d.getHours() % 12 || 12;
  return `${h}:${String(d.getMinutes()).padStart(2, "0")} ${d.getHours() < 12 ? "AM" : "PM"}`;
}

/** "d MMM" */
export const dayMonth = (d: Date) => `${d.getDate()} ${MONTHS[d.getMonth()]}`;

/** "EEEE d MMM" */
export const weekdayDayMonth = (d: Date) => `${WEEKDAYS[d.getDay()]} ${d.getDate()} ${MONTHS[d.getMonth()]}`;

const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

/** "Today", "Yesterday" or "EEEE d MMM". */
export function dayLabel(d: Date): string {
  const now = new Date();
  if (sameDay(d, now)) return "Today";
  const y = new Date(now);
  y.setDate(now.getDate() - 1);
  if (sameDay(d, y)) return "Yesterday";
  return weekdayDayMonth(d);
}

/** Meeting.clock: M:SS, or H:MM:SS from an hour. */
export function clock(t: number): string {
  const s = Math.max(0, Math.floor(t));
  const pad = (n: number) => String(n).padStart(2, "0");
  return s >= 3600 ? `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}` : `${Math.floor(s / 60)}:${pad(s % 60)}`;
}

export const nowApple = () => Date.now() / 1000 - APPLE_EPOCH_OFFSET;

export const meetingDuration = (m: Meeting) => (m.end ?? nowApple()) - m.start;
export const meetingWords = (m: Meeting) => m.segments.reduce((n, s) => n + s.text.split(" ").filter(Boolean).length, 0);
export const meetingTranscript = (m: Meeting) =>
  [...m.segments]
    .sort((a, b) => a.time - b.time)
    .map((s) => `[${clock(s.time)}] ${s.speaker}: ${s.text}`)
    .join("\n");
export const meetingStart = (m: Meeting) => fromAppleDate(m.start);

/** Swift `String.capitalized`: first letter of every word upper, the rest lower. */
export const capitalized = (s: string) => s.toLowerCase().replace(/(^|[^\p{L}\p{N}'])(\p{L})/gu, (_, a, b) => a + b.toUpperCase());

export const formatNumber = (n: number) => n.toLocaleString("en-US");

// ------------------------------------------------------------------------------------------------ app info

const appCache = new Map<string, AppInfo | null>();
const appPending = new Map<string, Promise<AppInfo | null>>();

function loadApp(bundle: string): Promise<AppInfo | null> {
  let p = appPending.get(bundle);
  if (!p) {
    p = api
      .appInfo(bundle)
      .catch(() => null)
      .then((info) => {
        appCache.set(bundle, info);
        return info;
      });
    appPending.set(bundle, p);
  }
  return p;
}

/** Fallback name like appDisplayName: last component of the bundle id. */
export const bundleFallbackName = (bundle: string) => bundle.split(".").pop() || bundle;

/** App name (without ".app") and icon for a bundle id, fetched once and cached. */
export function useAppInfo(bundle: string | undefined | null): { name: string; icon?: string } {
  const [, force] = useState(0);
  useEffect(() => {
    if (bundle && !appCache.has(bundle)) {
      let live = true;
      loadApp(bundle).then(() => live && force((n) => n + 1));
      return () => {
        live = false;
      };
    }
  }, [bundle]);
  if (!bundle) return { name: "" };
  const info = appCache.get(bundle);
  return { name: info?.name?.replace(/\.app$/, "") || bundleFallbackName(bundle), icon: info?.icon };
}

/** Resolve names for many bundles at once (usage panels). */
export function useAppNames(bundles: string[]): Record<string, string> {
  const [, force] = useState(0);
  const key = bundles.join("\n");
  useEffect(() => {
    const missing = bundles.filter((b) => !appCache.has(b));
    if (!missing.length) return;
    let live = true;
    Promise.all(missing.map(loadApp)).then(() => live && force((n) => n + 1));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  const out: Record<string, string> = {};
  for (const b of bundles) out[b] = appCache.get(b)?.name?.replace(/\.app$/, "") || bundleFallbackName(b);
  return out;
}
