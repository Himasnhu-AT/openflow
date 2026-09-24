import { useMemo, useState, type ReactNode } from "react";
import { AudioLines, Check, Copy, Pencil, Search, Trash2 } from "lucide-react";
import { api, fromAppleDate, type Dictation } from "../lib/api";
import { dayLabel, formatNumber, timeHMA, useAppInfo } from "../lib/format";
import { useStore } from "../lib/store";
import { AutoTextArea, CardList, Dot, IconButton, PageHeader, PrimaryButton, RowCard, SecondaryButton, SectionLabel, useFlash } from "../components/ui";

export function HomePage() {
  const { snap } = useStore();
  const [query, setQuery] = useState("");

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? snap.history.filter((d) => d.text.toLowerCase().includes(q)) : snap.history;
  }, [query, snap.history]);

  const grouped = useMemo(() => {
    const out: [string, Dictation[]][] = [];
    for (const d of filtered) {
      const label = dayLabel(fromAppleDate(d.date));
      if (out.length && out[out.length - 1][0] === label) out[out.length - 1][1].push(d);
      else out.push([label, [d]]);
    }
    return out;
  }, [filtered]);

  const clips = useMemo(() => new Set(snap.clipsPresent), [snap.clipsPresent]);

  return (
    <div className="page" style={{ flexDirection: "row", gap: 28, alignItems: "stretch" }}>
      <div className="page">
        <PageHeader title="Dictation" subtitle="Hold ⌘; anywhere to talk. Everything below stays on this Mac." />
        <div className="search" style={{ marginBottom: 18 }}>
          <Search size={14} strokeWidth={2.2} />
          <input placeholder="Search dictations" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
        </div>
        {filtered.length === 0 ? (
          <div className="empty">{snap.history.length === 0 ? "No dictations yet. Hold ⌘; and say something." : "No matches."}</div>
        ) : (
          <div className="scroll">
            <div style={{ display: "flex", flexDirection: "column", gap: 22, paddingBottom: 20 }}>
              {grouped.map(([label, items]) => (
                <div key={label} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
                  <SectionLabel text={label} />
                  <CardList>
                    {items.map((d) => (
                      <HistoryRow key={d.id} d={d} hasClip={clips.has(d.id)} />
                    ))}
                  </CardList>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
      <div style={{ width: 250, flex: "none", paddingTop: 74, display: "flex", flexDirection: "column", gap: 14 }}>
        <div className="panel" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <BigStat value={formatNumber(snap.stats.words)} label="total words" />
          <BigStat value={String(snap.wpm)} label="wpm" />
          <BigStat value={String(snap.streak)} label="day streak" />
        </div>
        <div className="panel" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 16, fontWeight: 600 }}>Engines</div>
          <EngineLine name={`Parakeet ${snap.config.asrVersion}`} ok={snap.speechReady} />
          <EngineLine
            name={
              snap.activeBackend === "ollama" ? `${snap.config.ollamaModel} (Ollama)` : snap.config.model.split(".gguf").join("")
            }
            ok={snap.polishReady}
          />
        </div>
      </div>
    </div>
  );
}

function BigStat({ value, label }: { value: string; label: string }) {
  return (
    <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
      <span style={{ fontFamily: "var(--serif)", fontSize: 28, fontWeight: 500 }}>{value}</span>
      <span style={{ fontSize: 15, color: "var(--secondary)" }}>{label}</span>
    </div>
  );
}

function EngineLine({ name, ok }: { name: string; ok: boolean }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
      <Dot color={ok ? "var(--green)" : "var(--gray)"} />
      <span style={{ fontSize: 13, color: "var(--secondary)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{name}</span>
    </div>
  );
}

function HistoryRow({ d, hasClip }: { d: Dictation; hasClip: boolean }) {
  const [copied, flash] = useFlash();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const app = useAppInfo(d.app);
  const shown = d.correction ?? d.text;

  const meta: ReactNode[] = [];
  const sep = (k: string) => <span key={k}>·</span>;
  if (d.app) meta.push(<span key="app">{app.name}</span>);
  if (d.tone) meta.push(sep("s1"), <span key="tone">{d.tone}</span>);
  if (d.seconds > 0) meta.push(sep("s2"), <span key="sec">{Math.trunc(d.seconds)}s</span>);
  if (d.correction != null) meta.push(sep("s3"), <span key="corr" style={{ color: "var(--accent)" }}>corrected</span>);
  if (hasClip) meta.push(sep("s4"), <AudioLines key="clip" size={11} strokeWidth={2.2} />);

  return (
    <RowCard
      actions={
        editing ? undefined : (
          <>
            <IconButton
              icon={Pencil}
              help="Correct: save what this should have said"
              onClick={() => {
                setDraft(shown);
                setEditing(true);
              }}
            />
            <IconButton
              icon={copied ? Check : Copy}
              help="Copy"
              onClick={() => {
                api.copyText(shown).catch(() => {});
                flash();
              }}
            />
            <IconButton icon={Trash2} help="Delete" onClick={() => api.deleteDictation(d.id)} />
          </>
        )
      }
    >
      <span style={{ width: 70, flex: "none", fontSize: 13, color: "var(--secondary)" }}>{timeHMA(fromAppleDate(d.date))}</span>
      <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {editing ? (
          <>
            <div style={{ fontSize: 12, color: "var(--tertiary)" }}>Heard: {d.raw}</div>
            <AutoTextArea className="text-editor" value={draft} onChange={setDraft} minHeight={56 + 12} autoFocus />
            <div style={{ display: "flex", gap: 8, paddingTop: 4 }}>
              <PrimaryButton
                title="Save correction"
                onClick={() => {
                  api.setCorrection(d.id, draft);
                  setEditing(false);
                }}
              />
              <SecondaryButton title="Cancel" onClick={() => setEditing(false)} />
            </div>
          </>
        ) : (
          <>
            <div className="selectable" style={{ fontSize: 15, whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>
              {shown}
            </div>
            <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 11, color: "var(--tertiary)" }}>{meta}</div>
          </>
        )}
      </div>
    </RowCard>
  );
}
