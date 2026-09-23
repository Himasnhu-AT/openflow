import { useState } from "react";
import { Trash2 } from "lucide-react";
import { useStore } from "../lib/store";
import { CardList, IconButton, PageHeader, PrimaryButton, RowCard, SecondaryButton, TextField } from "../components/ui";

export function DictionaryPage() {
  const { snap, setConfig } = useStore();
  const [adding, setAdding] = useState(false);
  const [newTerm, setNewTerm] = useState("");
  const dict = snap.config.dictionary;

  const add = () => {
    const t = newTerm.trim();
    if (!t) return;
    if (!dict.includes(t)) setConfig((c) => ({ ...c, dictionary: [t, ...c.dictionary] }));
    setNewTerm("");
    setAdding(false);
  };

  return (
    <div className="page">
      <PageHeader
        title="Dictionary"
        subtitle="Names and jargon spelled exactly like this. Sound-alikes in the transcript get replaced."
        trailing={<PrimaryButton title="Add new" onClick={() => setAdding(true)} />}
      />
      {adding && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, paddingBottom: 16 }}>
          <TextField
            value={newTerm}
            onChange={setNewTerm}
            onSubmit={add}
            autoFocus
            placeholder="Term, or Term: sound-alike, sound-alike (e.g. Zentro: zentrow, zen tro)"
            style={{ flex: 1 }}
          />
          <PrimaryButton title="Save" onClick={add} />
          <SecondaryButton
            title="Cancel"
            onClick={() => {
              setAdding(false);
              setNewTerm("");
            }}
          />
        </div>
      )}
      {dict.length === 0 ? (
        <div style={{ color: "var(--secondary)" }}>No terms yet.</div>
      ) : (
        <div className="scroll">
          <CardList>
            {dict.map((term) => (
              <RowCard
                key={term}
                actions={
                  <IconButton icon={Trash2} help="Delete" onClick={() => setConfig((c) => ({ ...c, dictionary: c.dictionary.filter((x) => x !== term) }))} />
                }
              >
                <span className="row-main" style={{ fontSize: 15 }}>
                  {term}
                </span>
              </RowCard>
            ))}
          </CardList>
        </div>
      )}
    </div>
  );
}
