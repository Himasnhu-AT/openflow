import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { AudioLines, BookA, Brain, ChartColumn, CircleHelp, Cpu, ALargeSmall, Mic, Settings, UsersRound, type LucideIcon } from "lucide-react";
import "./styles.css";
import { api, events } from "./lib/api";
import { StoreContext, useSnapshotStore } from "./lib/store";
import { DownloadsProvider } from "./lib/downloads";
import { Dot } from "./components/ui";
import { HomePage } from "./pages/Home";
import { MeetingsPage } from "./pages/Meetings";
import { InsightsPage } from "./pages/Insights";
import { DictionaryPage } from "./pages/Dictionary";
import { StylePage } from "./pages/Style";
import { MemoryPage } from "./pages/Memory";
import { ModelsPage } from "./pages/Models";
import { SettingsPage } from "./pages/Settings";
import { forwardErrors } from "./lib/errors";

forwardErrors("main");

type Page = "Home" | "Meetings" | "Insights" | "Dictionary" | "Style" | "Memory" | "Models" | "Settings";

const ICONS: Record<Page, LucideIcon> = {
  Home: Mic,
  Meetings: UsersRound,
  Insights: ChartColumn,
  Dictionary: BookA,
  Style: ALargeSmall,
  Memory: Brain,
  Models: Cpu,
  Settings: Settings,
};
const MAIN: Page[] = ["Home", "Meetings", "Insights", "Dictionary", "Style", "Memory", "Models"];
const ALL: Page[] = [...MAIN, "Settings"];

function pageFrom(raw: string | null | undefined): Page | null {
  if (!raw) return null;
  return ALL.find((p) => p.toLowerCase() === raw.trim().toLowerCase()) ?? null;
}

function NavItem({ title, icon: Icon, selected, onClick }: { title: string; icon: LucideIcon; selected: boolean; onClick: () => void }) {
  return (
    <button className={"nav-item" + (selected ? " selected" : "")} onClick={onClick}>
      <span className="nav-icon">
        <Icon size={18} strokeWidth={1.9} />
      </span>
      <span>{title}</span>
    </button>
  );
}

function App() {
  const store = useSnapshotStore();
  // `?page=Models` lets the browser dev build open on a page, like `--settings Models`.
  const [page, setPage] = useState<Page>(() => pageFrom(new URLSearchParams(location.search).get("page")) ?? "Home");

  useEffect(() => {
    const un = events.navigate((p) => {
      const next = pageFrom(p);
      if (next) setPage(next);
    });
    // `--settings <Page>` on launch: the navigate event can fire before this listener exists, so ask once.
    api.initialPage().then((p) => {
      const next = pageFrom(p);
      if (next) setPage(next);
    }, () => {});
    return () => void un.then((f) => f());
  }, []);

  if (!store) return <div className="app" data-tauri-drag-region />;
  const s = store.snap;
  const dotColor = s.speechReady && s.polishReady ? "var(--green)" : s.speechReady ? "var(--yellow)" : "var(--gray)";

  return (
    <StoreContext.Provider value={store}>
      <div className="drag-strip" data-tauri-drag-region />
      <div className="app">
        <nav className="sidebar">
          <div className="brand" data-tauri-drag-region>
            <AudioLines size={21} strokeWidth={2.4} />
            <span className="brand-name">OpenFlow</span>
            <span className="pill-local">Local</span>
          </div>
          {MAIN.map((p) => (
            <NavItem key={p} title={p} icon={ICONS[p]} selected={page === p} onClick={() => setPage(p)} />
          ))}
          <div className="sidebar-spacer" />
          <div className="sidebar-divider" />
          <NavItem title="Settings" icon={ICONS.Settings} selected={page === "Settings"} onClick={() => setPage("Settings")} />
          <NavItem title="Help" icon={CircleHelp} selected={false} onClick={() => api.openPath("readme")} />
          <div className="status-footer">
            <Dot color={dotColor} />
            <span className="status-text">{s.status}</span>
          </div>
        </nav>
        <div className="content-wrap">
          <main className="content-card">
            <div className="content">
              {page === "Home" && <HomePage />}
              {page === "Meetings" && <MeetingsPage />}
              {page === "Insights" && <InsightsPage />}
              {page === "Dictionary" && <DictionaryPage />}
              {page === "Style" && <StylePage />}
              {page === "Memory" && <MemoryPage />}
              {page === "Models" && <ModelsPage />}
              {page === "Settings" && <SettingsPage />}
            </div>
          </main>
        </div>
      </div>
    </StoreContext.Provider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <DownloadsProvider>
      <App />
    </DownloadsProvider>
  </React.StrictMode>,
);
