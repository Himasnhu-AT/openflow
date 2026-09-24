/*
THESIS: OpenFlow is one chord, ⌘;. The page is made of keyboard, not SaaS chrome. It refuses the dark hero with a glowing screenshot and a six-card feature grid.
OWN-WORLD: an anodized cobalt plate with sculpted PBT keycaps (top face, darker skirt, key travel). Light-grey alphas carry reading, charcoal mods carry structure, and lime marks only ⌘, ; and Return. Archivo is stretched wide for headlines and narrowed for legends.
STORY: hold the keys and watch messy speech land cleaned up in the right voice for each app. Then learn that it's private, fast and open source, and download it.
FIRST VIEWPORT: headline on the left with the Return-key Download button. On the right, a Mac text field above two giant ⌘ and ; caps you can hold, including with your real keyboard.
FORM: #5 The Keycap. Staging: a colorway sheet. Seed 42a5f6e6.
*/

import Image from "next/image";
import type { CSSProperties, ReactNode } from "react";
import Demo from "@/components/Demo";
import Gallery from "@/components/Gallery";
import ToneSwitcher from "@/components/ToneSwitcher";
import { LINKS } from "@/lib/site";

/** A static key with a main legend on top and a sub-legend on the front face. */
function LegendKey({
  kind = "alpha",
  size,
  legend,
  sub,
}: {
  kind?: "alpha" | "mod" | "accent" | "blank";
  size: string;
  legend: ReactNode;
  sub: ReactNode;
}) {
  return (
    <li className={`key key--${kind} ${size}`}>
      <span className="cap">
        <span className="legend">{legend}</span>
        <span className="sublegend">{sub}</span>
      </span>
    </li>
  );
}

const FIXES: { legend: string; sub: string; w?: "w15" | "w2" }[] = [
  { legend: "“um, so, uh…”", sub: "gone", w: "w2" },
  { legend: "“Tuesday, no wait, Thursday”", sub: "Thursday", w: "w2" },
  { legend: "“new line”", sub: "↵" },
  { legend: "“bullet point milk”", sub: "- milk", w: "w15" },
  { legend: "“sam at acme dot com”", sub: "sam@acme.com", w: "w2" },
  { legend: "“twenty dollars”", sub: "$20", w: "w15" },
  { legend: "“zen tro”", sub: "Zentro, from your dictionary", w: "w15" },
  { legend: "“fire emoji”", sub: "🔥" },
  { legend: "“…actually scratch that”", sub: "sentence deleted", w: "w2" },
  { legend: "“three things: …”", sub: "1. 2. 3.", w: "w15" },
];

const swatch = (c: string) => ({ "--c": c }) as CSSProperties;

export default function Home() {
  return (
    <>
      <a className="skip" href="#main">
        Skip to content
      </a>

      <header className="plate plate--hero">
        <nav className="nav" aria-label="Main">
          <a className="brand" href="#top" aria-label="OpenFlow home">
            <span className="key key--mod key--tiny" aria-hidden="true">
              <span className="cap">⌘;</span>
            </span>
            <span className="brand__word">OpenFlow</span>
          </a>
          <ul className="nav__links">
            {[
              ["F1", "What it fixes", "#fixes"],
              ["F2", "Private", "#private"],
              ["F3", "Meetings", "#meetings"],
              ["F4", "Open source", "#open"],
            ].map(([fn, label, href]) => (
              <li key={fn}>
                <a className="key key--mod key--small key--fn" href={href}>
                  <span className="cap">
                    <span className="fn">{fn}</span>
                    {label}
                  </span>
                </a>
              </li>
            ))}
          </ul>
          <a className="key key--accent key--small" href={LINKS.download}>
            <span className="cap">Download</span>
          </a>
        </nav>

        <div className="hero" id="top">
          <div className="hero__copy">
            <h1 className="display">
              Hold <span className="chord">⌘;</span> and&nbsp;talk.
            </h1>
            <p className="lede">
              Your words land where your cursor is: cleaned up, in your voice, and they never leave your Mac.
            </p>
            <div className="cta-row">
              <a className="key key--accent key--return" href={LINKS.download}>
                <span className="cap">
                  <span className="legend legend--big">Download for Mac</span>
                  <span className="sublegend">macOS 15+ · Apple Silicon · open source</span>
                </span>
              </a>
              <a className="key key--mod" href={LINKS.source}>
                <span className="cap">
                  <span className="legend">View source</span>
                  <span className="sublegend">BSD 3-Clause</span>
                </span>
              </a>
            </div>
          </div>

          <Demo />
        </div>
      </header>

      <main id="main">
        <section className="plate plate--alpha" id="fixes" aria-labelledby="fixes-h">
          <div className="wrap">
            <h2 className="headline" id="fixes-h">
              Say it how you say it.
              <br />
              It lands how you meant it.
            </h2>
            <p className="body">
              Fillers disappear, self-corrections resolve, and spoken commands turn into formatting. Each key shows what
              you say on top and what gets typed on the front.
            </p>
            <ul className="sheet" aria-label="Examples of what OpenFlow fixes">
              {FIXES.map((f) => (
                <LegendKey key={f.legend} size={`key--sheet ${f.w ?? ""}`} legend={f.legend} sub={f.sub} />
              ))}
              <LegendKey
                kind="mod"
                size="key--sheet w2"
                legend="Hold, or quick-tap"
                sub="tap starts, tap stops · Esc cancels"
              />
            </ul>
          </div>
        </section>

        <section className="plate plate--cobalt" id="tones" aria-labelledby="tones-h">
          <div className="wrap split">
            <div>
              <h2 className="headline" id="tones-h">
                One chord. The right voice in every app.
              </h2>
              <p className="body body--plate">
                Messages gets a text. Mail gets an email. Your terminal gets exactly what you said. Tones are
                plain-language instructions you can edit, and <kbd>⌘</kbd>
                <kbd>⇧</kbd>
                <kbd>;</kbd> switches to each app&apos;s alternate.
              </p>
            </div>
            <ToneSwitcher />
          </div>
        </section>

        <section className="plate plate--alpha" id="private" aria-labelledby="private-h">
          <div className="wrap">
            <h2 className="headline" id="private-h">
              Your Mac is the server.
            </h2>
            <p className="body">
              Speech recognition and cleanup run on your Mac. No account, no audio uploaded, and it keeps working on a
              plane. Cloud models are opt-in, use your own key, and fall back to local if anything fails.
            </p>
            <ol className="row" aria-label="What happens when you release the keys">
              <LegendKey kind="mod" size="key--row" legend="Mic" sub="16 kHz, on-device" />
              <LegendKey size="key--row" legend="Parakeet" sub="speech → text · ~70 ms per 3.8 s clip" />
              <LegendKey size="key--row" legend="Rules" sub="fillers, commands · 0.6 ms" />
              <LegendKey size="key--row" legend="Qwen3.5" sub="tone and grammar · llama.cpp" />
              <LegendKey kind="accent" size="key--row" legend="⌘V" sub="pasted at your cursor" />
              <LegendKey kind="blank" size="key--row" legend="Cloud" sub="off unless you turn it on" />
            </ol>
            <p className="footnote">
              Timings measured on an Apple Silicon Mac: transcription of a 3.8 s clip with Parakeet v2, and the rule pass
              on a 60-word dictation. Words also appear while you talk, so the transcript is ready the moment you let go.
            </p>
          </div>
        </section>

        <section className="plate plate--cobalt" id="meetings" aria-labelledby="meetings-h">
          <div className="wrap split split--wide">
            <div>
              <span className="key key--mod key--tiny key--tag">
                <span className="cap">Beta</span>
              </span>
              <h2 className="headline" id="meetings-h">
                Meeting notes, no bot in the call.
              </h2>
              <p className="body body--plate">
                When Zoom, Meet, Teams or FaceTime starts using your mic, OpenFlow offers to take notes. It never starts
                on its own. You and the other side are transcribed separately, and when the call ends you get a summary,
                the decisions, the action items and your own to-dos.
              </p>
              <ul className="caps">
                <LegendKey size="key--cap-fact" legend="No audio kept" sub="only the transcript" />
                <LegendKey size="key--cap-fact" legend="50+ languages" sub="other side, auto-detected" />
                <LegendKey size="key--cap-fact" legend="Search · Markdown" sub="every meeting, exportable" />
              </ul>
            </div>
            <figure className="window well">
              <Image
                src="/assets/app-meetings.jpg"
                width={1600}
                height={926}
                sizes="(max-width: 960px) 100vw, 700px"
                alt="OpenFlow's Meetings page showing notes for a sample call: summary, decisions, action items and a to-do checklist."
              />
              <figcaption>The real app, with a sample meeting.</figcaption>
            </figure>
          </div>
        </section>

        <section className="plate plate--alpha" id="yours" aria-labelledby="yours-h">
          <div className="wrap">
            <h2 className="headline" id="yours-h">
              It learns how you write.
            </h2>
            <p className="body">
              Add names and jargon to your dictionary and they get boosted inside the speech model itself. Tell it who
              you are once. When you fix a dictation, that correction becomes an example of your style.
            </p>
            <Gallery />
          </div>
        </section>

        <section className="plate plate--mod" id="open" aria-labelledby="open-h">
          <div className="wrap split">
            <div>
              <h2 className="headline" id="open-h">
                Open source. Measured on your own voice.
              </h2>
              <p className="body body--mod">
                Every rule, prompt and model is in the repo, and you can swap any of them. Before and after a change,{" "}
                <code>--eval</code> replays the dictations you corrected and reports word error rate, so you know
                whether the change helped.
              </p>
              <ul className="caps">
                <LegendKey size="key--cap-fact" legend="BSD 3-Clause" sub="use it, fork it, ship it" />
                <LegendKey size="key--cap-fact" legend="Rust · React · Tauri" sub="plus a small Swift speech helper" />
                <LegendKey size="key--cap-fact" legend="110 self-tests" sub="no models needed" />
              </ul>
              <a className="key key--alpha" href={LINKS.source}>
                <span className="cap">
                  <span className="legend">Read the source</span>
                  <span className="sublegend">GitHub</span>
                </span>
              </a>
            </div>
            <pre className="terminal" aria-label="Build from source">
              <code>
                <span className="c"># build it yourself</span>
                {"\n"}
                <span className="p">$</span> brew install llama.cpp{"\n"}
                <span className="p">$</span> pnpm install{"\n"}
                <span className="p">$</span> ./scripts/build.sh --install{"\n\n"}
                <span className="c"># rule checks, no models needed</span>
                {"\n"}
                <span className="p">$</span> OpenFlow --selftest{"\n"}
                selftest: 110 passed, 0 failed{"\n\n"}
                <span className="c"># replay your corrected dictations</span>
                {"\n"}
                <span className="p">$</span> OpenFlow --eval
              </code>
            </pre>
          </div>
        </section>

        <section className="plate plate--alpha plate--tight" id="needs" aria-labelledby="needs-h">
          <div className="wrap">
            <h2 className="headline headline--small" id="needs-h">
              What you need
            </h2>
            <ul className="row" aria-label="Requirements">
              <LegendKey size="key--row" legend="A Mac" sub="macOS 15 or later, Apple Silicon" />
              <LegendKey
                size="key--row"
                legend="llama.cpp"
                sub={
                  <>
                    <code>brew install llama.cpp</code> for cleanup; without it, text is pasted verbatim
                  </>
                }
              />
              <LegendKey
                size="key--row"
                legend="Models"
                sub="downloaded once in the app: speech ~600 MB, cleanup 1.3–5.7 GB"
              />
              <LegendKey kind="blank" size="key--row" legend="Windows · Linux" sub="in progress, not released yet" />
            </ul>
          </div>
        </section>

        <section className="plate plate--cobalt plate--close" aria-labelledby="close-h">
          <div className="wrap close">
            <h2 className="display display--close" id="close-h">
              Stop typing everything.
            </h2>
            <a className="key key--accent key--return key--return-big" href={LINKS.download}>
              <span className="cap">
                <span className="legend legend--big">Download for Mac</span>
                <span className="sublegend">then hold ⌘; anywhere</span>
              </span>
            </a>
          </div>
        </section>
      </main>

      <footer className="footer">
        <div className="wrap footer__row">
          <span className="brand__word brand__word--small">OpenFlow</span>
          <span className="colorway" aria-label="Colorway: cobalt plate, PBT alpha, charcoal modifier, lime accent">
            <i style={swatch("var(--plate-cobalt)")} />
            <i style={swatch("var(--cap-alpha)")} />
            <i style={swatch("var(--cap-mod)")} />
            <i style={swatch("var(--cap-accent)")} />
            <span>Colorway: Cobalt / Lime · PBT</span>
          </span>
          <span>Open source under BSD 3-Clause.</span>
          <a href={LINKS.source}>Source</a>
          <a href="#private">Privacy: everything runs on your Mac</a>
        </div>
      </footer>
    </>
  );
}
