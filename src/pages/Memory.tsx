import { useStore } from "../lib/store";
import { PageHeader } from "../components/ui";

export function MemoryPage() {
  const { snap, setConfig } = useStore();
  return (
    <div className="page" style={{ gap: 14 }}>
      <PageHeader
        title="Memory"
        subtitle="Background the polish model always knows about you. Used for context only, never inserted into your text."
      />
      <textarea
        className="memory-editor"
        value={snap.config.memory}
        onChange={(e) => setConfig({ memory: e.target.value }, { debounce: true })}
      />
      <div style={{ fontSize: 12, color: "var(--tertiary)", flex: "none" }}>
        Example: "I'm Taylor, based in Melbourne. I run a small design studio called Acme Studio and I use Australian spelling."
      </div>
    </div>
  );
}
