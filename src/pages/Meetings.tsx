import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { CircleX, Copy, Search, Share, Trash2 } from "lucide-react";
import { api, type Meeting } from "../lib/api";
import { saveDialog } from "../lib/bridge";
import { clock, dayMonth, meetingDuration, meetingStart, meetingTranscript, meetingWords } from "../lib/format";
import { useStore } from "../lib/store";
import { Checkbox, IconButton, LiveBadge, PageHeader, PrimaryButton, SecondaryButton, Spinner, Tabs } from "../components/ui";

type Tab = "Summary" | "Transcript" | "Notes";

// ------------------------------------------------------------------------------------------------ search

/** Case- and diacritic-insensitive fold that remembers where each folded char came from. */
function fold(text: string): { s: string; map: number[] } {
  let s = "";
  const map: number[] = [];
  let i = 0;
  for (const ch of text) {
    const f = ch.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
    for (let k = 0; k < f.length; k++) map.push(i);
    s += f;
    i += ch.length;
  }
  map.push(i);
  return { s, map };
}

/** Range of the first match in the original string, or null. */
function findInsensitive(text: string, query: string): [number, number] | null {
  const q = fold(query).s;
  if (!q) return null;
  const t = fold(text);
  const at = t.s.indexOf(q);
  if (at < 0) return null;
  const lo = t.map[at];
  const endFold = at + q.length;
  // End = start of the next original char after the last matched folded char.
  let hi = t.map[endFold] ?? text.length;
  if (hi <= lo) hi = lo + 1;
  return [lo, hi];
}

/** Offset by n user-perceived characters (code points), clamped. */
function offsetBy(text: string, from: number, n: number): number {
  let i = from;
  if (n < 0) {
    for (let k = 0; k < -n && i > 0; k++) i -= i >= 2 && /[\uDC00-\uDFFF]/.test(text[i - 1]) && /[\uD800-\uDBFF]/.test(text[i - 2]) ? 2 : 1;
  } else {
    for (let k = 0; k < n && i < text.length; k++) i += /[\uD800-\uDBFF]/.test(text[i]) ? 2 : 1;
  }
  return i;
}

interface Hit {
  key: string;
  meeting: Meeting;
  field: "Title" | "Summary" | "Notes" | "Transcript";
  snippet: string;
}

function searchMeetings(meetings: Meeting[], query: string): Hit[] {
  const q = query.trim();
  if ([...q].length < 2) return [];
  const out: Hit[] = [];
  for (const m of meetings) {
    const fields: [Hit["field"], string][] = [
      ["Title", m.title],
      ["Summary", m.summary],
      ["Notes", m.notes],
      ...m.segments.map((s) => ["Transcript", `${s.speaker}: ${s.text}`] as [Hit["field"], string]),
    ];
    let perMeeting = 0;
    for (const [field, text] of fields) {
      const r = findInsensitive(text, q);
      if (!r) continue;
      const lo = offsetBy(text, r[0], -60);
      const hi = offsetBy(text, r[1], 60);
      const snippet = (lo > 0 ? "…" : "") + text.slice(lo, hi).replace(/\n/g, " ") + (hi < text.length ? "…" : "");
      out.push({ key: `${m.id}-${out.length}`, meeting: m, field, snippet });
      perMeeting += 1;
      if (perMeeting >= 3) break;
    }
    if (out.length >= 30) break;
  }
  return out;
}

function Highlighted({ text, query }: { text: string; query: string }) {
  const r = findInsensitive(text, query.trim());
  if (!r) return <>{text}</>;
  return (
    <>
      {text.slice(0, r[0])}
      <b style={{ color: "var(--text)", fontWeight: 700 }}>{text.slice(r[0], r[1])}</b>
      {text.slice(r[1])}
    </>
  );
}

// ------------------------------------------------------------------------------------------------ page

export function MeetingsPage() {
  const { snap } = useStore();
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("Summary");
  const [query, setQuery] = useState("");
  const requested = useRef(new Set<string>());

  const hits = useMemo(() => searchMeetings(snap.meetings, query), [snap.meetings, query]);
  const currentId = selected ?? snap.liveMeetingId ?? snap.meetings[0]?.id;
  const current = snap.meetings.find((m) => m.id === currentId);

  return (
    <div className="page">
      <PageHeader
        title="Meetings"
        subtitle="Beta. Transcribed and summarised locally (or via your own cloud key). Audio is never stored."
        trailing={
          snap.liveMeetingId != null ? (
            <SecondaryButton title="Stop" destructive onClick={() => api.stopMeeting()} />
          ) : (
            <PrimaryButton title="Start notes now" onClick={() => api.startMeeting()} />
          )
        }
      />
      {snap.meetings.length === 0 ? (
        <div className="empty">No meetings yet. OpenFlow offers to take notes when a call starts, or press Start notes now.</div>
      ) : (
        <div style={{ display: "flex", gap: 20, flex: 1, minHeight: 0, position: "relative" }}>
          <div style={{ width: 240, flex: "none", display: "flex", flexDirection: "column", gap: 10 }}>
            <div className="search">
              <Search size={14} strokeWidth={2.2} />
              <input placeholder="Search meetings" value={query} onChange={(e) => setQuery(e.target.value)} spellCheck={false} />
              {query && (
                <button onClick={() => setQuery("")} style={{ color: "var(--tertiary)", display: "flex" }} aria-label="Clear">
                  <CircleX size={14} fill="currentColor" color="var(--card)" />
                </button>
              )}
            </div>
            <div className="scroll">
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {snap.meetings.map((m) => (
                  <MeetingListItem key={m.id} m={m} on={m.id === current?.id} onClick={() => setSelected(m.id)} />
                ))}
              </div>
            </div>
          </div>
          {current ? (
            <Detail
              key={current.id}
              m={current}
              tab={tab}
              setTab={setTab}
              requested={requested.current}
              onDeleted={() => setSelected(null)}
            />
          ) : (
            <div className="spacer" />
          )}
          {hits.length > 0 && (
            <div className="search-results scroll">
              {hits.map((h) => (
                <Fragment key={h.key}>
                  <button
                    className="search-hit"
                    onClick={() => {
                      setSelected(h.meeting.id);
                      setTab(h.field === "Notes" ? "Notes" : h.field === "Transcript" ? "Transcript" : "Summary");
                      setQuery("");
                    }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                        {h.meeting.title}
                      </span>
                      <span className="badge" style={{ padding: "2px 6px" }}>
                        {h.field}
                      </span>
                      <span className="spacer" />
                      <span style={{ fontSize: 11, color: "var(--tertiary)", flex: "none" }}>{dayMonth(meetingStart(h.meeting))}</span>
                    </div>
                    <div className="clamp2" style={{ fontSize: 12, color: "var(--secondary)" }}>
                      <Highlighted text={h.snippet} query={query} />
                    </div>
                  </button>
                  <div className="divider" />
                </Fragment>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function MeetingListItem({ m, on, onClick }: { m: Meeting; on: boolean; onClick: () => void }) {
  return (
    <button className={"meeting-item" + (on ? " on" : "")} onClick={onClick}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <span style={{ fontSize: 13, fontWeight: 500, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", flex: 1 }}>{m.title}</span>
        {m.end == null && <LiveBadge />}
      </div>
      <div style={{ fontSize: 11, color: "var(--tertiary)" }}>
        {dayMonth(meetingStart(m))} · {Math.floor(meetingDuration(m) / 60)} min · {meetingWords(m)} words
      </div>
    </button>
  );
}

// ------------------------------------------------------------------------------------------------ detail

function Detail({
  m,
  tab,
  setTab,
  requested,
  onDeleted,
}: {
  m: Meeting;
  tab: Tab;
  setTab: (t: Tab) => void;
  requested: Set<string>;
  onDeleted: () => void;
}) {
  const { snap, updateMeeting } = useStore();
  const live = m.end == null;
  const summarizing = snap.summarizing.includes(m.id);

  const copy = () => {
    const text = tab === "Summary" ? notesPlainText(m) : tab === "Notes" ? m.notes : meetingTranscript(m);
    api.copyText(text).catch(() => {});
  };
  const exportMd = async () => {
    const path = await saveDialog(m.title.replace(/[/:]/g, "-") + ".md");
    if (!path) return;
    const md = await api.meetingMarkdown(m.id);
    await api.writeTextFile(path, md);
  };

  return (
    <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 12, minHeight: 0 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <input
          className="title-field"
          value={m.title}
          placeholder="Title"
          spellCheck={false}
          onChange={(e) => updateMeeting({ ...m, title: e.target.value, titleAuto: false }, { debounce: true })}
        />
        {live && <LiveBadge />}
      </div>
      <div style={{ display: "flex", alignItems: "flex-end", gap: 4 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <Tabs<Tab>
            items={[
              ["Summary", "Summary"],
              ["Transcript", "Transcript"],
              ["Notes", "Notes"],
            ]}
            value={tab}
            onChange={setTab}
          />
        </div>
        <div style={{ display: "flex", paddingBottom: 18, marginLeft: 8 }}>
          <IconButton icon={Copy} help={`Copy ${tab.toLowerCase()}`} onClick={copy} />
          <IconButton icon={Share} help="Export as Markdown" onClick={exportMd} />
          {!live && (
            <IconButton
              icon={Trash2}
              help="Delete meeting"
              onClick={() => {
                api.deleteMeeting(m.id);
                onDeleted();
              }}
            />
          )}
        </div>
      </div>
      {tab === "Summary" && <SummaryTab m={m} summarizing={summarizing} requested={requested} />}
      {tab === "Notes" && (
        <textarea
          className="notes-editor"
          value={m.notes}
          onChange={(e) => updateMeeting({ ...m, notes: e.target.value }, { debounce: true })}
        />
      )}
      {tab === "Transcript" && <TranscriptTab m={m} />}
    </div>
  );
}

function SummaryTab({ m, summarizing, requested }: { m: Meeting; summarizing: boolean; requested: Set<string> }) {
  const live = m.end == null;
  // Notes are written without asking: once per meeting per view, when a finished meeting with a transcript has none.
  useEffect(() => {
    if (live || m.summary || !m.segments.length || requested.has(m.id)) return;
    requested.add(m.id);
    api.summarizeMeeting(m.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [m.id]);

  return (
    <div className="scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
        {!m.summary ? (
          summarizing ? (
            <div style={{ display: "flex", alignItems: "center", gap: 8, color: "var(--secondary)" }}>
              <Spinner /> Writing the notes…
            </div>
          ) : (
            <div style={{ color: "var(--secondary)" }}>
              {m.segments.length === 0 ? (live ? "Listening… notes appear a few minutes in." : "Nothing was transcribed.") : "Notes are on their way."}
            </div>
          )
        ) : (
          <>
            <NotesView m={m} />
            <div style={{ display: "flex", alignItems: "center", gap: 8, paddingBottom: 12 }}>
              {summarizing ? (
                <>
                  <Spinner />
                  <span style={{ fontSize: 12, color: "var(--tertiary)" }}>Updating…</span>
                </>
              ) : live ? (
                <span style={{ fontSize: 12, color: "var(--tertiary)" }}>Updates every few minutes while the meeting runs.</span>
              ) : (
                <SecondaryButton title="Rewrite notes" onClick={() => api.summarizeMeeting(m.id)} />
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function TranscriptTab({ m }: { m: Meeting }) {
  const ref = useRef<HTMLDivElement>(null);
  const live = m.end == null;
  const lines = useMemo(() => [...m.segments].sort((a, b) => a.time - b.time), [m.segments]);
  useLayoutEffect(() => {
    if (live && ref.current) ref.current.scrollTo({ top: ref.current.scrollHeight, behavior: "smooth" });
  }, [m.segments.length, live]);
  return (
    <div className="scroll" ref={ref}>
      <div style={{ display: "flex", flexDirection: "column", gap: 10, paddingBottom: 12 }}>
        {lines.length === 0 && (
          <div style={{ color: "var(--secondary)" }}>{live ? "Listening… lines appear as each person finishes a sentence." : "Nothing was transcribed."}</div>
        )}
        {lines.map((s) => (
          <div key={s.id} style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
            <span style={{ width: 44, flex: "none", textAlign: "right", fontSize: 11, color: "var(--tertiary)", fontVariantNumeric: "tabular-nums", paddingTop: 2 }}>
              {clock(s.time)}
            </span>
            <span style={{ width: 40, flex: "none", fontSize: 12, fontWeight: 600, color: s.speaker === "You" ? "var(--accent)" : "var(--them)", paddingTop: 1 }}>
              {s.speaker}
            </span>
            <span className="selectable" style={{ fontSize: 14, minWidth: 0 }}>
              {s.text}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------------------------------------ NotesView

interface Section {
  title: string;
  items: string[];
}

function sections(summary: string): Section[] {
  const out: Section[] = [];
  let title = "";
  let items: string[] = [];
  const flush = () => {
    if (title) out.push({ title, items });
    items = [];
  };
  for (const raw of summary.split("\n")) {
    if (!raw) continue;
    const line = raw.trim();
    if (line.startsWith("#")) {
      flush();
      title = line.replace(/^[# ]+|[# ]+$/g, "");
      continue;
    }
    if (line.startsWith("-") || line.startsWith("•") || line.startsWith("*")) {
      const t = line.slice(1).trim();
      if (t && t.toLowerCase() !== "none") items.push(t);
    } else if (line) items.push(line);
  }
  flush();
  return out;
}

export function notesPlainText(m: Meeting): string {
  let out = m.title + "\n";
  for (const s of sections(m.summary)) {
    if (s.title.toLowerCase() === "your to-dos") continue;
    out += `\n${s.title}\n` + s.items.map((i) => "• " + i).join("\n") + "\n";
  }
  if (m.todos.length) out += "\nYour to-dos\n" + m.todos.map((t) => (t.done ? "☑ " : "☐ ") + t.text).join("\n") + "\n";
  return out;
}

/** Inline markdown (bold, italic, code, links) with the "Name:" lead of an action item made bold. */
function inline(s: string): ReactNode {
  let text = s;
  const lead = /^[A-Z][\p{L}\p{N}_' ]{0,24}:/u.exec(text);
  if (lead && !text.startsWith("**")) text = "**" + lead[0].slice(0, -1) + ":**" + text.slice(lead[0].length);
  return markdownInline(text);
}

function markdownInline(text: string): ReactNode {
  const out: ReactNode[] = [];
  const re = /\*\*(.+?)\*\*|__(.+?)__|\*(.+?)\*|(?<![\p{L}\p{N}_])_(.+?)_(?![\p{L}\p{N}_])|`(.+?)`|\[(.+?)\]\((.+?)\)/gu;
  let last = 0;
  let k = 0;
  for (let mm = re.exec(text); mm; mm = re.exec(text)) {
    if (mm.index > last) out.push(text.slice(last, mm.index));
    if (mm[1] ?? mm[2]) out.push(<strong key={k++}>{markdownInline(mm[1] ?? mm[2])}</strong>);
    else if (mm[3] ?? mm[4]) out.push(<em key={k++}>{mm[3] ?? mm[4]}</em>);
    else if (mm[5]) out.push(<code key={k++} style={{ fontFamily: "var(--mono)", fontSize: "0.92em" }}>{mm[5]}</code>);
    else out.push(<span key={k++} style={{ color: "var(--accent)" }}>{mm[6]}</span>);
    last = mm.index + mm[0].length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}

function NotesView({ m }: { m: Meeting }) {
  const { updateMeeting } = useStore();
  const secs = sections(m.summary);
  const hasTodoSection = secs.some((s) => s.title.toLowerCase() === "your to-dos");

  const todoSection = (
    <div key="todos" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div style={{ fontSize: 17, fontWeight: 600 }}>Your to-dos</div>
      {m.todos.map((t) => (
        <label key={t.id} style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
          <Checkbox
            on={t.done}
            onChange={(v) => updateMeeting({ ...m, todos: m.todos.map((x) => (x.id === t.id ? { ...x, done: v } : x)) })}
          />
          <span style={{ fontSize: 14, color: t.done ? "var(--tertiary)" : "var(--text)", textDecoration: t.done ? "line-through" : "none" }}>
            {inline(t.text)}
          </span>
        </label>
      ))}
    </div>
  );

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 22 }}>
      {secs.map((s, i) =>
        s.title.toLowerCase() === "your to-dos" ? (
          m.todos.length ? <Fragment key={i}>{todoSection}</Fragment> : null
        ) : s.items.length ? (
          <div key={i} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ fontSize: 17, fontWeight: 600 }}>{s.title}</div>
            {s.items.map((item, j) => (
              <div key={j} style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
                <span style={{ color: "var(--accent)" }}>•</span>
                <span className="selectable" style={{ fontSize: 14 }}>
                  {inline(item)}
                </span>
              </div>
            ))}
          </div>
        ) : null,
      )}
      {!hasTodoSection && m.todos.length > 0 && todoSection}
    </div>
  );
}
