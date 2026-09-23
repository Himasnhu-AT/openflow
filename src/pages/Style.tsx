import { useEffect, useRef, useState } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import { api, type AppInfo, type Config, type ToneConfig } from "../lib/api";
import { capitalized, useAppInfo } from "../lib/format";
import { useStore } from "../lib/store";
import { CardList, IconButton, PageHeader, Picker, PrimaryButton, RowCard, SecondaryButton, SectionLabel, Tabs, TextField } from "../components/ui";

type Tab = "tones" | "apps";

export function StylePage() {
  const { snap, setConfig } = useStore();
  const [tab, setTab] = useState<Tab>("tones");
  const [selected, setSelected] = useState<string>(() => snap.config.defaultTones.tone);
  const [rename, setRename] = useState<string>(() => snap.config.defaultTones.tone);

  const addTone = () => {
    const tones = snap.config.tones;
    let name = "new tone";
    let n = 2;
    while (name in tones) name = `new tone ${n++}`;
    setConfig((c) => ({ ...c, tones: { ...c.tones, [name]: "Tone: " } }));
    setSelected(name);
    setRename(name);
  };

  return (
    <div className="page">
      <PageHeader
        title="Style"
        subtitle="How your dictation reads, and which style each app gets."
        trailing={tab === "tones" ? <PrimaryButton title="Add tone" onClick={addTone} /> : undefined}
      />
      <Tabs<Tab>
        items={[
          ["tones", "Tones"],
          ["apps", "Apps"],
        ]}
        value={tab}
        onChange={setTab}
      />
      {tab === "tones" ? (
        <TonesTab
          selected={selected}
          select={(n) => {
            setSelected(n);
            setRename(n);
          }}
          rename={rename}
          setRename={setRename}
        />
      ) : (
        <AppsTab />
      )}
    </div>
  );
}

// ------------------------------------------------------------------------------------------------ tones

function TonesTab({
  selected,
  select,
  rename,
  setRename,
}: {
  selected: string;
  select: (n: string) => void;
  rename: string;
  setRename: (n: string) => void;
}) {
  const { snap, setConfig } = useStore();
  const c = snap.config;
  const names = Object.keys(c.tones).sort();

  const renameTone = (old: string) => {
    const next = rename.trim().toLowerCase();
    if (!next || next === old || next in c.tones) return;
    setConfig((cfg) => {
      const tones: Record<string, string> = {};
      for (const [k, v] of Object.entries(cfg.tones)) if (k !== old) tones[k] = v;
      tones[next] = cfg.tones[old];
      const swap = (t: string) => (t === old ? next : t);
      const appTones: Record<string, ToneConfig> = {};
      for (const [k, v] of Object.entries(cfg.appTones)) appTones[k] = { tone: swap(v.tone), alt: swap(v.alt) };
      return { ...cfg, tones, appTones, defaultTones: { tone: swap(cfg.defaultTones.tone), alt: swap(cfg.defaultTones.alt) } };
    });
    select(next);
  };

  return (
    <div className="scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 22, paddingBottom: 20 }}>
        <div className="tone-grid">
          {names.map((name) => {
            const instruction = c.tones[name] ?? "";
            return (
              <button key={name} className={"tone-card" + (selected === name ? " on" : "")} onClick={() => select(name)}>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <span style={{ fontSize: 17, fontWeight: 600, flex: 1 }}>{capitalized(name)}</span>
                  {c.defaultTones.tone === name ? <span className="badge">Default</span> : c.defaultTones.alt === name ? <span className="badge">Alt</span> : null}
                </div>
                <div className="clamp4" style={{ fontSize: 12, color: "var(--secondary)" }}>
                  {instruction || "Verbatim — pasted exactly as transcribed."}
                </div>
              </button>
            );
          })}
        </div>
        {selected in c.tones && (
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <TextField value={rename} onChange={setRename} onSubmit={() => renameTone(selected)} placeholder="Tone name" width={220} />
              <div className="spacer" />
              <SecondaryButton title="Set as default" onClick={() => setConfig((cfg) => ({ ...cfg, defaultTones: { ...cfg.defaultTones, tone: selected } }))} />
              <SecondaryButton title="Set as alt (⌘⇧;)" onClick={() => setConfig((cfg) => ({ ...cfg, defaultTones: { ...cfg.defaultTones, alt: selected } }))} />
              <SecondaryButton
                title="Delete"
                destructive
                onClick={() => {
                  let after = c.defaultTones.tone;
                  setConfig((cfg) => {
                    const tones = { ...cfg.tones };
                    delete tones[selected];
                    after = cfg.defaultTones.tone;
                    return { ...cfg, tones };
                  });
                  select(after);
                }}
              />
            </div>
            <div style={{ fontSize: 12, color: "var(--secondary)" }}>Instruction given to the polish model after the cleanup rules. Leave empty for verbatim.</div>
            <textarea
              className="tone-editor"
              spellCheck={false}
              value={c.tones[selected] ?? ""}
              onChange={(e) => {
                const v = e.target.value;
                setConfig((cfg) => ({ ...cfg, tones: { ...cfg.tones, [selected]: v } }), { debounce: true });
              }}
            />
          </div>
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------------------------------------ apps

function TonePickers({ value, onChange, names }: { value: ToneConfig; onChange: (v: ToneConfig) => void; names: string[] }) {
  const options = names.map((n) => [n, capitalized(n)] as [string, string]);
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, flex: "none" }}>
      <Picker width={130} value={value.tone} options={options} onChange={(tone) => onChange({ ...value, tone })} />
      <span style={{ fontSize: 12, color: "var(--tertiary)" }}>alt</span>
      <Picker width={130} value={value.alt} options={options} onChange={(alt) => onChange({ ...value, alt })} />
    </div>
  );
}

function AppRow({ bundle, names }: { bundle: string; names: string[] }) {
  const { snap, setConfig } = useStore();
  const info = useAppInfo(bundle);
  const value = snap.config.appTones[bundle] ?? snap.config.defaultTones;
  return (
    <RowCard
      actions={
        <IconButton
          icon={Trash2}
          help="Remove"
          onClick={() =>
            setConfig((c) => {
              const appTones = { ...c.appTones };
              delete appTones[bundle];
              return { ...c, appTones };
            })
          }
        />
      }
    >
      {info.icon && <img src={info.icon} width={26} height={26} alt="" style={{ flex: "none" }} />}
      <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 2 }}>
        <span style={{ fontSize: 15 }}>{info.name}</span>
        <span style={{ fontSize: 11, color: "var(--tertiary)" }}>{bundle}</span>
      </div>
      <TonePickers names={names} value={value} onChange={(v) => setConfig((c) => ({ ...c, appTones: { ...c.appTones, [bundle]: v } }))} />
    </RowCard>
  );
}

function RunningAppMenu({ onPick }: { onPick: (bundle: string) => void }) {
  const [open, setOpen] = useState(false);
  const [apps, setApps] = useState<AppInfo[] | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    api
      .runningApps()
      .then((list) => setApps(list.filter((a) => a.bundleId !== "com.jez.flow").sort((a, b) => a.name.localeCompare(b.name))))
      .catch(() => setApps([]));
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  return (
    <div ref={ref} style={{ position: "relative", width: 150, flex: "none" }}>
      <button className="menu-button" onClick={() => setOpen((o) => !o)}>
        <span>Running app…</span>
        <ChevronDown size={12} strokeWidth={2.5} />
      </button>
      {open && (
        <div className="menu" style={{ bottom: "calc(100% + 4px)", left: 0 }}>
          {apps == null && <div className="menu-item" style={{ color: "var(--tertiary)" }}>Loading…</div>}
          {apps?.map((a) => (
            <button
              key={a.bundleId}
              className="menu-item"
              onClick={() => {
                onPick(a.bundleId);
                setOpen(false);
              }}
            >
              {a.icon ? <img src={a.icon} width={16} height={16} alt="" /> : <span style={{ width: 16 }} />}
              {a.name}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function AppsTab() {
  const { snap, setConfig } = useStore();
  const [newBundle, setNewBundle] = useState("");
  const c = snap.config;
  const names = Object.keys(c.tones).sort();

  const addApp = () => {
    const b = newBundle.trim();
    if (!b) return;
    setConfig((cfg: Config) => ({ ...cfg, appTones: { ...cfg.appTones, [b]: cfg.defaultTones } }));
    setNewBundle("");
  };

  return (
    <div className="scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 18, paddingBottom: 20 }}>
        <div style={{ fontSize: 13, color: "var(--secondary)" }}>⌘; uses the app's tone, ⌘⇧; its alternate. Apps not listed use the default.</div>
        <SectionLabel text="Default" />
        <CardList>
          <RowCard key="default">
            <span className="row-main" style={{ fontSize: 15 }}>
              All other apps
            </span>
            <TonePickers names={names} value={c.defaultTones} onChange={(v) => setConfig({ defaultTones: v })} />
          </RowCard>
        </CardList>
        <SectionLabel text="Per app" />
        {Object.keys(c.appTones).length > 0 && (
          <CardList>
            {Object.keys(c.appTones)
              .sort()
              .map((bundle) => (
                <AppRow key={bundle} bundle={bundle} names={names} />
              ))}
          </CardList>
        )}
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <RunningAppMenu onPick={setNewBundle} />
          <TextField value={newBundle} onChange={setNewBundle} onSubmit={addApp} placeholder="or type a bundle identifier" style={{ flex: 1 }} />
          <PrimaryButton title="Add" onClick={addApp} />
        </div>
      </div>
    </div>
  );
}
