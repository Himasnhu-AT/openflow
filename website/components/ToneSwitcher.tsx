"use client";

import { useState } from "react";

type ToneId = "messages" | "mail" | "notes" | "terminal";

// Illustrative output for one sentence. Messages, Mail and Terminal use their default tones; casual is a
// tone the user assigns.
const TONES: Record<ToneId, { key: string; name: string; text: string; mono: boolean }> = {
  messages: { key: "Messages", name: "supercasual · Messages", text: "hey Jordan thanks for the deck can we move the review to thursday?", mono: false },
  mail: { key: "Mail", name: "professional · Mail", text: "Hey Jordan, thanks for the deck. Can we move the review to Thursday?", mono: false },
  notes: { key: "Notes, set to casual", name: "casual · Notes (your setting)", text: "hey Jordan, thanks for the deck. can we move the review to thursday?", mono: false },
  terminal: { key: "Terminal", name: "verbatim · Terminal", text: "hey jordan um thanks for the deck can we move the review to thursday", mono: true },
};
const ORDER: ToneId[] = ["messages", "mail", "notes", "terminal"];

export default function ToneSwitcher() {
  const [id, setId] = useState<ToneId>("messages");
  const tone = TONES[id];
  return (
    <div className="tones">
      <p className="tones__said">
        <span className="tones__label">You said</span>
        “hey jordan um thanks for the deck can we move the review to thursday”
      </p>
      <div className="tones__keys" role="group" aria-label="App">
        {ORDER.map((t) => (
          <button
            key={t}
            className={`key key--alpha key--small${id === t ? " is-on" : ""}`}
            aria-pressed={id === t}
            type="button"
            onClick={() => setId(t)}
          >
            <span className="cap">{TONES[t].key}</span>
          </button>
        ))}
      </div>
      <div className={`tones__out${tone.mono ? " is-mono" : ""}`} aria-live="polite">
        <span className="tones__label">{tone.name}</span>
        <p>{tone.text}</p>
      </div>
      <p className="note note--plate">
        Illustrative output. Messages, Mail and Terminal use their default tones; casual is a tone you can give any app.
      </p>
    </div>
  );
}
