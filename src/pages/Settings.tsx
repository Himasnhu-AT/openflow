import { useEffect, useState, type ReactNode } from "react";
import { api, type Config, type InputDevice } from "../lib/api";
import { useOllamaStatus } from "../lib/ollama";
import { useStore } from "../lib/store";
import { CardList, PageHeader, Picker, RowCard, SecondaryButton, SectionLabel, Switch, TextField } from "../components/ui";

const EVAL_COMMAND = "/Applications/OpenFlow.app/Contents/MacOS/OpenFlow --eval";

function SettingRow({ title, subtitle, children }: { title: string; subtitle: string; children: ReactNode }) {
  return (
    <RowCard>
      <div className="row-main" style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <span style={{ fontSize: 15 }}>{title}</span>
        <span className="clamp2" style={{ fontSize: 12, color: "var(--tertiary)" }}>
          {subtitle}
        </span>
      </div>
      {children}
    </RowCard>
  );
}

const Keycap = ({ text }: { text: string }) => <span className="keycap">{text}</span>;

type BoolKey = { [K in keyof Config]: Config[K] extends boolean ? K : never }[keyof Config];
type StringKey = { [K in keyof Config]: Config[K] extends string ? K : never }[keyof Config];

export function SettingsPage() {
  const { snap, setConfig } = useStore();
  const c = snap.config;
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const { status: ollama } = useOllamaStatus();
  const onOllama = c.llmBackend !== "llamacpp";

  useEffect(() => {
    api.getLaunchAtLogin().then(setLaunchAtLogin).catch(() => {});
    api.inputDevices().then(setDevices).catch(() => {});
  }, []);

  const toggle = (key: BoolKey) => <Switch on={c[key]} onChange={(v) => setConfig({ [key]: v } as Partial<Config>)} />;
  const text = (key: StringKey, width = 260, type = "text", placeholder?: string) => (
    <TextField value={c[key]} type={type} placeholder={placeholder} width={width} onChange={(v) => setConfig({ [key]: v } as Partial<Config>, { debounce: true })} />
  );
  const picker = (key: StringKey, options: [string, string][], width = 190) => (
    <Picker width={width} value={c[key]} options={options} onChange={(v) => setConfig({ [key]: v } as Partial<Config>)} />
  );
  const pct = ((c.tapThreshold - 0.2) / 0.8) * 100;

  return (
    <div className="page scroll page-scroll">
      <div style={{ display: "flex", flexDirection: "column", gap: 18, paddingBottom: 24 }}>
        <PageHeader title="Settings" />

        <SectionLabel text="Hotkey" />
        <CardList>
          <SettingRow key="dictate" title="Dictate" subtitle="Hold to talk, quick tap to toggle, Esc cancels">
            <Keycap text="⌘ ;" />
          </SettingRow>
          <SettingRow key="alt" title="Alternate tone" subtitle="Same, but uses the app's alt tone">
            <Keycap text="⌘ ⇧ ;" />
          </SettingRow>
          <SettingRow key="tap" title="Tap threshold" subtitle="Presses shorter than this switch to toggle mode">
            <div style={{ display: "flex", alignItems: "center", gap: 8, flex: "none" }}>
              <input
                className="slider"
                type="range"
                min={0.2}
                max={1}
                step={0.05}
                value={c.tapThreshold}
                style={{ width: 160, ["--pct" as string]: `${pct}%` }}
                onChange={(e) => setConfig({ tapThreshold: Math.round(Number(e.target.value) * 100) / 100 }, { debounce: true })}
              />
              <span style={{ width: 48, fontSize: 13, color: "var(--secondary)", fontVariantNumeric: "tabular-nums" }}>{c.tapThreshold.toFixed(2)} s</span>
            </div>
          </SettingRow>
        </CardList>

        <SectionLabel text="App" />
        <CardList>
          <SettingRow key="wal" title="Open window at launch" subtitle="Off keeps it to the Dock and menu bar, handy with Launch at login">
            {toggle("windowAtLaunch")}
          </SettingRow>
          <SettingRow key="lal" title="Launch at login" subtitle="Keep OpenFlow in the menu bar">
            <Switch
              on={launchAtLogin}
              onChange={(on) => {
                setLaunchAtLogin(on);
                api.setLaunchAtLogin(on).catch(() => api.getLaunchAtLogin().then(setLaunchAtLogin).catch(() => {}));
              }}
            />
          </SettingRow>
          {onOllama ? (
            <SettingRow key="ollama" title="Ollama URL" subtitle="Where the Ollama server listens. Pick the engine on the Models page">
              {text("ollamaUrl", 260, "text", "http://127.0.0.1:11434")}
            </SettingRow>
          ) : (
            <SettingRow key="llama" title="llama-server" subtitle="Path to the Homebrew binary">
              {text("llamaServerPath")}
            </SettingRow>
          )}
          <SettingRow key="cfg" title="Config folder" subtitle={snap.dataDir}>
            <SecondaryButton title="Open" onClick={() => api.openPath("config")} />
          </SettingRow>
        </CardList>

        <SectionLabel text="Microphone" />
        <CardList>
          <SettingRow key="mic" title="Input device" subtitle="The built-in mic is silent while the lid is closed; AirPods are fine but narrowband">
            {picker(
              "microphone",
              [["", "System default"], ["builtin", "Built-in microphone"], ...devices.filter((d) => !d.isBuiltIn).map((d) => [d.uid, d.name] as [string, string])],
              230,
            )}
          </SettingRow>
        </CardList>

        <SectionLabel text="While dictating" />
        <CardList>
          <SettingRow key="pill" title="Floating pill" subtitle="Waveform at the bottom of the screen while listening">
            {toggle("overlay")}
          </SettingRow>
          <SettingRow key="sounds" title="Sounds" subtitle="Soft cue on start and stop">
            {toggle("sounds")}
          </SettingRow>
          <SettingRow key="live" title="Live text in the field" subtitle="Types what it hears as you talk, then swaps in the polished text">
            {toggle("liveText")}
          </SettingRow>
          <SettingRow key="field" title="Read the field you're typing in" subtitle="Window title and existing text help with names and mid-sentence continuation">
            {toggle("fieldContext")}
          </SettingRow>
        </CardList>

        <SectionLabel text="Processing" />
        <CardList>
          <SettingRow
            key="cloud"
            title="Use cloud models (Groq)"
            subtitle="Whisper large v3 turbo for speech, Llama 3.3 70B for cleanup and notes. Audio and text leave this Mac; live text stays local; falls back to local on failure"
          >
            {toggle("cloudEnabled")}
          </SettingRow>
          <SettingRow
            key="cd"
            title="Cloud for dictation"
            subtitle="Long = dictations over 15 s, where the local 4B takes 5-7 s to polish; short ones stay local (as accurate, better on names, private)"
          >
            {picker("cloudDictation", [
              ["off", "Off"],
              ["long", "Long dictations only"],
              ["all", "All"],
            ])}
          </SettingRow>
          <SettingRow
            key="cm"
            title="Cloud for meetings"
            subtitle="Recommended: Whisper large v3 turbo handles compressed call audio better, and a 70B model writes the notes"
          >
            {toggle("cloudMeetings")}
          </SettingRow>
          <SettingRow key="key" title="Groq API key" subtitle="console.groq.com → API Keys. Stored in config.json on this Mac">
            {text("cloudAPIKey", 260, "password", "gsk_…")}
          </SettingRow>
          <SettingRow key="asr" title="Cloud speech model" subtitle="Groq model id">
            {text("cloudASRModel")}
          </SettingRow>
          <SettingRow key="llm" title="Cloud text model" subtitle="Groq model id">
            {text("cloudLLMModel")}
          </SettingRow>
        </CardList>

        <SectionLabel text="Meetings" />
        <CardList>
          <SettingRow key="detect" title="Detect calls" subtitle="Offers to take notes when a meeting app or call tab is using the mic">
            {toggle("meetingDetect")}
          </SettingRow>
          <SettingRow key="zh" title="Chinese script" subtitle="Cantonese and Mandarin come out Simplified from the speech model">
            {picker("chineseScript", [
              ["traditional", "Traditional"],
              ["simplified", "As detected (Simplified)"],
            ])}
          </SettingRow>
          <SettingRow
            key="far"
            title="Far side (them)"
            subtitle="Whisper large v3 turbo on the built-in speech server: any language, best on compressed call audio. Download the model under Models"
          >
            {picker("meetingFarEngine", [
              ["whisper", "Whisper large v3 turbo"],
              ["parakeet", "Parakeet"],
            ])}
          </SettingRow>
          {onOllama ? (
            <SettingRow key="notes" title="Notes model" subtitle="An Ollama model for meeting notes; a bigger one writes better notes">
              {picker("ollamaNotesModel", [
                ["", "Same as dictation model"],
                ...(ollama?.models ?? []).filter((m) => m.name !== c.ollamaModel).map((m) => [m.name, m.name] as [string, string]),
                // The configured model may not be pulled yet (the default 9B, say): say so instead of a bare tag.
                ...(ollama && c.ollamaNotesModel && !ollama.models.some((m) => m.name === c.ollamaNotesModel)
                  ? [[c.ollamaNotesModel, `${c.ollamaNotesModel} (not pulled)`] as [string, string]]
                  : []),
              ])}
            </SettingRow>
          ) : (
            <SettingRow key="notes" title="Notes model" subtitle="A bigger local model loaded only while notes are written">
              {picker("notesModel", [
                ["", "Same as polish model"],
                ...snap.installedModels.filter((f) => f !== c.model).map((f) => [f, f.split(".gguf").join("")] as [string, string]),
              ])}
            </SettingRow>
          )}
          <SettingRow key="you" title="Your side (you)" subtitle="Parakeet; Whisper or SenseVoice only for longer non-English speech">
            {picker("meetingEngine", [
              ["auto", "Auto (any language)"],
              ["english", "English only"],
            ])}
          </SettingRow>
        </CardList>

        <SectionLabel text="Names" />
        <CardList>
          <SettingRow
            key="contacts"
            title="Use Contacts and Calendar"
            subtitle="Names of people, companies and today's attendees are recognised better (asks for access once)"
          >
            {toggle("useContacts")}
          </SettingRow>
          <SettingRow
            key="learned"
            title="Learned automatically"
            subtitle="Correcting a name with the pencil on Home adds it to the Dictionary with what was heard; names visible where you type are boosted per dictation"
          >
            <span style={{ fontSize: 13, color: "var(--secondary)", flex: "none" }}>{c.dictionary.length} terms</span>
          </SettingRow>
        </CardList>

        <SectionLabel text="Tuning" />
        <CardList>
          <SettingRow key="clips" title="Keep audio clips" subtitle="Saves each dictation's audio in the config folder, never leaves this Mac">
            {toggle("keepClips")}
          </SettingRow>
          <SettingRow
            key="corr"
            title="Corrections"
            subtitle={`${snap.correctionCount} saved · the pencil on a dictation teaches the polish model your style`}
          >
            <SecondaryButton title="Copy eval command" onClick={() => api.copyText(EVAL_COMMAND).catch(() => {})} />
          </SettingRow>
        </CardList>
      </div>
    </div>
  );
}
