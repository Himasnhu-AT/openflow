"use client";

import Image from "next/image";
import { useState } from "react";

type ShotId = "home" | "style" | "insights";

const SHOTS: Record<ShotId, { key: string; src: string; w: number; h: number; alt: string }> = {
  home: {
    key: "History",
    src: "/assets/app-home.jpg",
    w: 1540,
    h: 1250,
    alt: "OpenFlow's history of recent dictations, each with the app and tone it was written in.",
  },
  style: {
    key: "Tones",
    src: "/assets/app-style.jpg",
    w: 1600,
    h: 926,
    alt: "OpenFlow's Style page: editable tones (casual, professional, supercasual, verbatim) with the instruction for each.",
  },
  insights: {
    key: "Insights",
    src: "/assets/app-insights.jpg",
    w: 1600,
    h: 926,
    alt: "OpenFlow's Insights page: words per minute, total words, streak, and usage by app and tone.",
  },
};
const ORDER: ShotId[] = ["home", "style", "insights"];

export default function Gallery() {
  const [id, setId] = useState<ShotId>("home");
  const shot = SHOTS[id];
  return (
    <div className="gallery">
      <div className="gallery__keys" role="group" aria-label="App screens">
        {ORDER.map((s) => (
          <button
            key={s}
            className={`key key--mod key--small${id === s ? " is-on" : ""}`}
            aria-pressed={id === s}
            type="button"
            onClick={() => setId(s)}
          >
            <span className="cap">{SHOTS[s].key}</span>
          </button>
        ))}
      </div>
      <figure className="window window--light well well--light">
        <Image src={shot.src} width={shot.w} height={shot.h} alt={shot.alt} sizes="(max-width: 1040px) 100vw, 980px" />
        <figcaption>The real app, with sample data.</figcaption>
      </figure>
    </div>
  );
}
