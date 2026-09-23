import { capitalized, formatNumber, useAppNames } from "../lib/format";
import { useStore } from "../lib/store";
import { PageHeader, StatTile } from "../components/ui";

const extra = (text: string) => <div style={{ fontSize: 12, color: "var(--tertiary)" }}>{text}</div>;

export function InsightsPage() {
  const { snap } = useStore();
  const s = snap.stats;
  const names = useAppNames(Object.keys(s.perApp));
  return (
    <div className="page">
      <PageHeader title="Insights" subtitle="All-time, counted locally since you installed OpenFlow." />
      <div className="scroll">
        <div style={{ display: "flex", gap: 16, paddingBottom: 16 }}>
          <StatTile value={String(snap.wpm)} label="Words per minute" extra={extra("Speaking is ~3× faster than typing")} />
          <StatTile value={formatNumber(s.words)} label="Total words dictated" extra={extra(`${s.dictations} dictations`)} />
          <StatTile value={String(snap.streak)} label="Day streak" extra={extra(`${s.activeDays.length} active days`)} />
        </div>
        <div style={{ display: "flex", alignItems: "flex-start", gap: 16 }}>
          <UsagePanel title="App usage" rows={Object.entries(s.perApp).map(([k, v]) => [names[k], v])} />
          <UsagePanel title="Tone usage" rows={Object.entries(s.perTone).map(([k, v]) => [capitalized(k), v])} />
        </div>
      </div>
    </div>
  );
}

function UsagePanel({ title, rows }: { title: string; rows: [string, number][] }) {
  const total = Math.max(
    rows.reduce((n, r) => n + r[1], 0),
    1,
  );
  const sorted = [...rows].sort((a, b) => b[1] - a[1]).slice(0, 8);
  return (
    <div className="panel" style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 14 }}>
      <div style={{ fontSize: 20, fontWeight: 600 }}>{title}</div>
      {sorted.length === 0 && <div style={{ color: "var(--secondary)" }}>Nothing yet.</div>}
      {sorted.map(([name, count], i) => {
        const frac = count / total;
        return (
          <div key={name + i} style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <span style={{ width: 110, flex: "none", fontSize: 13, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{name}</span>
            <div style={{ flex: 1, height: 26, position: "relative", borderRadius: 6, background: "var(--window)" }}>
              <div
                style={{
                  position: "absolute",
                  inset: "0 auto 0 0",
                  width: `max(${frac * 100}%, 4px)`,
                  borderRadius: 6,
                  background: "rgba(158, 140, 250, 0.75)",
                }}
              />
              <span style={{ position: "absolute", left: 8, top: 0, bottom: 0, display: "flex", alignItems: "center", fontSize: 11, fontWeight: 600, color: "#fff" }}>
                {Math.trunc(frac * 100)}%
              </span>
            </div>
            <span style={{ width: 40, flex: "none", textAlign: "right", fontSize: 12, color: "var(--secondary)" }}>{count}</span>
          </div>
        );
      })}
    </div>
  );
}
