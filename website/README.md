# OpenFlow website

A static landing page with no build step. Open `index.html`, or serve the folder:

```sh
python3 -m http.server 8931 --directory website
```

Deploy by copying this folder to any static host (GitHub Pages, Netlify, Cloudflare Pages).

## Before publishing

- Set the real URLs in `app.js` (`LINKS.download`, `LINKS.source`). The HTML hrefs use the same placeholders, `https://github.com/REPLACE-ME/openflow…`, for visitors without JavaScript, so replace those too: `grep -rn REPLACE-ME website/`.
- Screenshots in `assets/` show the real app with synthetic sample data. Regenerate them after UI changes.
- The hero demo is simulated and labelled as such. Every number on the page was measured, and the footnote says how.

The design system is in `DESIGN.md`. Product facts and the claims the page may make are in `../PRODUCT.md`.
