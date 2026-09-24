import { useState, type ReactNode } from "react";
import { api, type CatalogModel, type OllamaCatalogModel, type OllamaStatus, type Snapshot } from "../lib/api";
import { useDownloads } from "../lib/downloads";
import { formatGB, useOllamaStatus } from "../lib/ollama";
import { useStore } from "../lib/store";
import { CardList, Dot, PageHeader, Picker, PrimaryButton, RowCard, SecondaryButton, SectionLabel, Segmented, useFlash } from "../components/ui";

const ActiveBadge = () => <span className="badge-active">Active</span>;

const INSTALL_COMMAND = "brew install ollama";

function Progress({ file, onCancel }: { file: string; onCancel: () => void }) {
  const { progress } = useDownloads();
  const p = progress[file];
  return (
    <>
      <div className={"progress" + (p == null ? " indeterminate" : "")} style={{ width: 120 }}>
        <div style={p == null ? undefined : { width: `${p * 100}%` }} />
      </div>
      <span style={{ width: 36, fontSize: 12, color: "var(--secondary)", fontVariantNumeric: "tabular-nums", textAlign: "center", flex: "none" }}>
        {p == null ? "—" : `${Math.trunc(p * 100)}%`}
      </span>
      <SecondaryButton title="Cancel" onClick={onCancel} />
    </>
  );
}

function ModelInfo({ label, meta, note, errorKey, children }: { label: string; meta: string; note?: string; errorKey: string; children?: ReactNode }) {
  const { errors } = useDownloads();
  return (
    <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 8, minWidth: 0 }}>
        <span style={{ fontSize: 15, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{label}</span>
        <span style={{ fontSize: 12, color: "var(--tertiary)", whiteSpace: "nowrap" }}>{meta}</span>
      </div>
      {note && <span style={{ fontSize: 12, color: "var(--secondary)" }}>{note}</span>}
      {children}
      {errors[errorKey] && <span style={{ fontSize: 12, color: "var(--red)" }}>{errors[errorKey]}</span>}
    </div>
  );
}

function Section({ label, subtitle, children }: { label: string; subtitle?: string; children: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        <SectionLabel text={label} />
        {subtitle && <span style={{ fontSize: 12, color: "var(--tertiary)" }}>{subtitle}</span>}
      </div>
      {children}
    </div>
  );
}

const ggufName = (file: string) => file.split(".gguf").join("");
/** Ollama reports "qwen3.5:4b" or "name:latest"; treat a bare name as ":latest". */
const tagKey = (name: string) => (name.includes(":") ? name : `${name}:latest`);

/** The line under the engine picker: what actually loaded. */
function activeEngineLine(snap: Snapshot): { text: string; dot: string } {
  const c = snap.config;
  const ready = snap.polishReady ? "var(--green)" : "var(--gray)";
  if (snap.activeBackend === null) return { text: "No polish engine — pasting verbatim", dot: "var(--gray)" };
  if (snap.activeBackend === "llamacpp") {
    if (c.llmBackend === "ollama") return { text: "Ollama unavailable — using llama.cpp", dot: "var(--orange)" };
    return { text: `Active: llama.cpp with ${ggufName(c.model)}`, dot: ready };
  }
  return { text: `Active: Ollama with ${c.ollamaModel}`, dot: ready };
}

function OllamaStatusRow({ status, error, onStarted }: { status: OllamaStatus | null; error: string | null; onStarted: () => void }) {
  const [starting, setStarting] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [copied, flash] = useFlash();

  let dot = "var(--gray)";
  let title: ReactNode = "Checking Ollama…";
  let detail: ReactNode = null;
  let action: ReactNode = null;

  if (!status && error) {
    dot = "var(--orange)";
    title = "Couldn't check Ollama";
    detail = error;
  } else if (status?.running) {
    dot = "var(--green)";
    title = `Ollama ${status.version ?? ""} at ${status.url}`.replace("  ", " ");
  } else if (status?.installed) {
    dot = "var(--orange)";
    title = "Ollama isn't running";
    detail = startError ?? status.error;
    action = (
      <PrimaryButton
        title={starting ? "Starting…" : "Start Ollama"}
        onClick={() => {
          if (starting) return;
          setStarting(true);
          setStartError(null);
          api
            .ollamaStart()
            .catch((e) => setStartError(String(e)))
            .finally(() => {
              setStarting(false);
              onStarted();
            });
        }}
      />
    );
  } else if (status) {
    dot = "var(--orange)";
    title = "Ollama isn't installed";
    detail = (
      <>
        Install Ollama: <span style={{ fontFamily: "var(--mono)", color: "var(--secondary)" }}>{INSTALL_COMMAND}</span>, or ollama.com/download
      </>
    );
    action = (
      <SecondaryButton
        title={copied ? "Copied" : "Copy command"}
        onClick={() => {
          api.copyText(INSTALL_COMMAND).catch(() => {});
          flash();
        }}
      />
    );
  }

  return (
    <RowCard key="status">
      <Dot color={dot} />
      <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <span style={{ fontSize: 15 }}>{title}</span>
        {detail && <span style={{ fontSize: 12, color: "var(--tertiary)" }}>{detail}</span>}
      </div>
      {action}
    </RowCard>
  );
}

export function ModelsPage() {
  const { snap, setConfig } = useStore();
  const dl = useDownloads();
  const { status, error: statusError, refresh } = useOllamaStatus();
  const c = snap.config;

  // llama.cpp (GGUF)
  const installed = snap.installedModels;
  const extra = installed.filter((f) => !snap.catalog.some((m) => m.file === f));
  const w = snap.whisperCatalog;
  const haveWhisper = snap.whisperModelInstalled ?? installed.includes(w.file);
  const use = (file: string) => (c.model === file ? <ActiveBadge /> : <SecondaryButton title="Use" onClick={() => api.setModel(file)} />);
  const ggufInfo = (m: CatalogModel, children?: ReactNode) => (
    <ModelInfo label={m.label} meta={`${m.gb.toFixed(1)} GB`} note={m.note} errorKey={m.file}>
      {children}
    </ModelInfo>
  );

  // Ollama
  const pulled = new Map((status?.models ?? []).map((m) => [tagKey(m.name), m]));
  const ollamaExtra = (status?.models ?? []).filter((m) => !snap.ollamaCatalog.some((x) => tagKey(x.name) === tagKey(m.name)));
  const useOllama = (tag: string) =>
    tagKey(c.ollamaModel) === tagKey(tag) ? <ActiveBadge /> : <SecondaryButton title="Use" onClick={() => setConfig({ ollamaModel: tag })} />;
  const ollamaState = (m: OllamaCatalogModel) =>
    m.name in dl.progress ? (
      <Progress file={m.name} onCancel={() => dl.cancelPull(m.name)} />
    ) : pulled.has(tagKey(m.name)) ? (
      useOllama(m.name)
    ) : (
      <PrimaryButton title="Pull" onClick={() => dl.pull(m.name)} />
    );

  const engine = activeEngineLine(snap);

  return (
    <div className="page scroll page-scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 24, paddingBottom: 24 }}>
        <PageHeader
          title="Models"
          subtitle="Everything runs on this Mac. Switching restarts the local server in a few seconds."
          trailing={<SecondaryButton title="Open models folder" onClick={() => api.openPath("models")} />}
        />

        <Section label="Engine">
          <CardList>
            <RowCard key="engine">
              <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                <span style={{ fontSize: 15 }}>Polish engine</span>
                <span style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 12, color: "var(--secondary)" }}>
                  <Dot color={engine.dot} size={6} />
                  {engine.text}
                </span>
              </div>
              <Segmented
                value={c.llmBackend === "llamacpp" ? "llamacpp" : "ollama"}
                options={[
                  ["ollama", "Ollama (recommended)"],
                  ["llamacpp", "llama.cpp"],
                ]}
                onChange={(v) => setConfig({ llmBackend: v })}
              />
            </RowCard>
          </CardList>
        </Section>

        <Section label="Ollama" subtitle="Cleans up dictation through a local Ollama server. Models are pulled into Ollama's own store.">
          <CardList>
            <OllamaStatusRow status={status} error={statusError} onStarted={refresh} />
            {snap.ollamaCatalog.map((m) => (
              <RowCard key={m.name}>
                <ModelInfo label={m.label} meta={`${m.name} · ${m.gb.toFixed(1)} GB`} note={m.note} errorKey={m.name} />
                {ollamaState(m)}
              </RowCard>
            ))}
            {ollamaExtra.map((m) => (
              <RowCard key={m.name}>
                <ModelInfo label={m.name} meta={formatGB(m.size)} errorKey={m.name} />
                {useOllama(m.name)}
              </RowCard>
            ))}
          </CardList>
        </Section>

        <Section label="llama.cpp (GGUF)" subtitle="Used when the engine is llama.cpp, and as the fallback when Ollama isn't available.">
          <CardList>
            {snap.catalog.map((m) => (
              <RowCard key={m.file}>
                {ggufInfo(m)}
                {m.file in dl.progress ? (
                  <Progress file={m.file} onCancel={() => dl.cancel(m.file)} />
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
        </Section>

        <Section label="Far side of calls">
          <CardList>
            <RowCard key="whisper">
              {ggufInfo(
                w,
                <span style={{ fontSize: 12, color: snap.whisperServerFound ? "var(--tertiary)" : "var(--orange)" }}>
                  {snap.whisperServerFound ? "Speech server ready" : "Speech server missing: reinstall OpenFlow"}
                </span>,
              )}
              {w.file in dl.progress ? (
                <Progress file={w.file} onCancel={() => dl.cancel(w.file)} />
              ) : haveWhisper ? (
                <ActiveBadge />
              ) : (
                <PrimaryButton title="Download" onClick={() => dl.start(w.file, w.url)} />
              )}
            </RowCard>
          </CardList>
        </Section>

        <Section label="Speech model">
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
                value={c.asrVersion}
                options={[
                  ["v2", "v2 English"],
                  ["v3", "v3 Multilingual"],
                ]}
                onChange={(v) => api.setAsrVersion(v)}
              />
            </RowCard>
          </CardList>
        </Section>
      </div>
    </div>
  );
}
