// Download progress (ModelDownloader's published state), app-wide so it survives page switches.
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { api, events } from "./api";

export interface Downloads {
  /** file → 0..1, or null while the size is unknown. Absent = not downloading. */
  progress: Record<string, number | null>;
  errors: Record<string, string>;
  start: (file: string, url: string) => void;
  cancel: (file: string) => void;
}

const Ctx = createContext<Downloads | null>(null);

export function DownloadsProvider({ children }: { children: ReactNode }) {
  const [progress, setProgress] = useState<Record<string, number | null>>({});
  const [errors, setErrors] = useState<Record<string, string>>({});

  useEffect(() => {
    const un = events.download((p) => {
      setProgress((prev) => {
        const next = { ...prev };
        if (p.done) delete next[p.file];
        else next[p.file] = p.progress;
        return next;
      });
      if (p.done) {
        setErrors((prev) => {
          const next = { ...prev };
          if (p.error) next[p.file] = p.error;
          else delete next[p.file];
          return next;
        });
      }
    });
    return () => void un.then((f) => f());
  }, []);

  const value: Downloads = {
    progress,
    errors,
    start: (file, url) => {
      if (file in progress) return;
      setProgress((p) => ({ ...p, [file]: 0 }));
      setErrors((e) => {
        const next = { ...e };
        delete next[file];
        return next;
      });
      api.downloadModel(file, url).catch((err) => {
        setProgress((p) => {
          const next = { ...p };
          delete next[file];
          return next;
        });
        setErrors((e) => ({ ...e, [file]: String(err) }));
      });
    },
    cancel: (file) => {
      api.cancelDownload(file).catch(() => {});
      setProgress((p) => {
        const next = { ...p };
        delete next[file];
        return next;
      });
    },
  };
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useDownloads(): Downloads {
  const d = useContext(Ctx);
  if (!d) throw new Error("useDownloads outside DownloadsProvider");
  return d;
}
