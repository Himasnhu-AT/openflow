# OpenFlow website

The landing page for OpenFlow, built with Next.js (App Router), React 19 and TypeScript. Package manager: pnpm.

```sh
cd website
pnpm install
pnpm dev          # http://localhost:3000
pnpm build        # production build (the page is statically prerendered)
pnpm lint
```

## Deploy

**Vercel (recommended):** import the repo and set **Root Directory** to `website`. The framework preset (Next.js), the build command and pnpm are detected automatically.

**Any Node host:** run `pnpm build && pnpm start`.

### Environment variables

Set these in your host's project settings. Until they are set, the page links to `REPLACE-ME` placeholders.

| Variable | What it is |
|---|---|
| `NEXT_PUBLIC_DOWNLOAD_URL` | Where "Download for Mac" goes, e.g. `https://github.com/<you>/openflow/releases/latest` |
| `NEXT_PUBLIC_SOURCE_URL` | The repository, e.g. `https://github.com/<you>/openflow` |
| `NEXT_PUBLIC_SITE_URL` | The deployed origin, used for absolute Open Graph URLs, e.g. `https://openflow.app` |

## Layout

- `app/page.tsx`: the page, server-rendered.
- `app/globals.css`: the keycap design system. Tokens and rules are documented in `DESIGN.md`.
- `components/Demo.tsx`: the hold-to-dictate hero demo (on-screen ⌘ ; keys or the real ⌘; chord).
- `components/ToneSwitcher.tsx`, `components/Gallery.tsx`: the other interactive parts.
- `public/assets/`: screenshots of the real app with synthetic sample data. Regenerate them after UI changes.

The claims the page may make are listed in `../PRODUCT.md`. Every number on the page was measured, and the hero demo is labelled as simulated.
