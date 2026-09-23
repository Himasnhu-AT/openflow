import type { ReactNode } from "react";
import { api, type CatalogModel } from "../lib/api";
import { useDownloads } from "../lib/downloads";
import { useStore } from "../lib/store";
import { CardList, PageHeader, Picker, PrimaryButton, RowCard, SecondaryButton, SectionLabel } from "../components/ui";

const ActiveBadge = () => <span className="badge-active">Active</span>;

function Progress({ file }: { file: string }) {
  const { progress, cancel } = useDownloads();
  const p = progress[file];
  return (
    <>
      <div className={"progress" + (p == null ? " indeterminate" : "")} style={{ width: 120 }}>
        <div style={p == null ? undefined : { width: `${p * 100}%` }} />
      </div>
      <span style={{ width: 36, fontSize: 12, color: "var(--secondary)", fontVariantNumeric: "tabular-nums", textAlign: "center", flex: "none" }}>
        {p == null ? "—" : `${Math.trunc(p * 100)}%`}
      </span>
      <SecondaryButton title="Cancel" onClick={() => cancel(file)} />
    </>
  );
}

function ModelInfo({ m, children }: { m: CatalogModel; children?: ReactNode }) {
  const { errors } = useDownloads();
  return (
    <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <span style={{ fontSize: 15, fontWeight: 600 }}>{m.label}</span>
        <span style={{ fontSize: 12, color: "var(--tertiary)" }}>{m.gb.toFixed(1)} GB</span>
      </div>
      <span style={{ fontSize: 12, color: "var(--secondary)" }}>{m.note}</span>
      {children}
      {errors[m.file] && <span style={{ fontSize: 12, color: "var(--red)" }}>{errors[m.file]}</span>}
    </div>
  );
}

export function ModelsPage() {
  const { snap } = useStore();
  const dl = useDownloads();
  const installed = snap.installedModels;
  const extra = installed.filter((f) => !snap.catalog.some((m) => m.file === f));
  const w = snap.whisperCatalog;
  const haveWhisper = snap.whisperModelInstalled ?? installed.includes(w.file);
  const use = (file: string) => (snap.config.model === file ? <ActiveBadge /> : <SecondaryButton title="Use" onClick={() => api.setModel(file)} />);

  return (
    <div className="page scroll page-scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 18, paddingBottom: 24 }}>
        <PageHeader
          title="Models"
          subtitle="Everything runs on this Mac. Switching restarts the local server in a few seconds."
          trailing={<SecondaryButton title="Open models folder" onClick={() => api.openPath("models")} />}
        />
        <SectionLabel text="Polish model" />
        <CardList>
          {snap.catalog.map((m) => (
            <RowCard key={m.file}>
              <ModelInfo m={m} />
              {m.file in dl.progress ? (
                <Progress file={m.file} />
              ) : installed.includes(m.file) ? (
                use(m.file)
              ) : (
                <PrimaryButton title="Download" onClick={() => dl.start(m.file, m.url)} />
              )}
            </RowCard>
          ))}
          {extra.map((f) => (
            <RowCard key={f}>
              <span className="row-main" style={{ fontSize: 15 }}>
                {f}
              </span>
              {use(f)}
            </RowCard>
          ))}
        </CardList>
        <SectionLabel text="Far side of calls" />
        <CardList>
          <RowCard key="whisper">
            <ModelInfo m={w}>
              <span style={{ fontSize: 12, color: snap.whisperServerFound ? "var(--tertiary)" : "var(--orange)" }}>
                {snap.whisperServerFound ? "whisper-server found" : "whisper-server not found: run brew install whisper-cpp"}
              </span>
            </ModelInfo>
            {w.file in dl.progress ? <Progress file={w.file} /> : haveWhisper ? <ActiveBadge /> : <PrimaryButton title="Download" onClick={() => dl.start(w.file, w.url)} />}
          </RowCard>
        </CardList>
        <SectionLabel text="Speech model" />
        <CardList>
          <RowCard key="parakeet">
            <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
              <span style={{ fontSize: 15, fontWeight: 600 }}>Parakeet TDT 0.6B</span>
              <span style={{ fontSize: 12, color: "var(--secondary)" }}>
                NVIDIA, on CoreML. v2 is English-only and most accurate; v3 covers 25 European languages.
              </span>
            </div>
            <Picker
              width={160}
              value={snap.config.asrVersion}
              options={[
                ["v2", "v2 English"],
                ["v3", "v3 Multilingual"],
              ]}
              onChange={(v) => api.setAsrVersion(v)}
            />
          </RowCard>
        </CardList>
      </div>
    </div>
  );
}
