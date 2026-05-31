# JIG.SH landing site

Astro marketing site for [jig-sh](https://github.com/bpcakes/jig-sh). Migrated from the root `landing.html` single-file page.

## Commands

```sh
cd landing
bun install   # or npm install
bun run dev
bun run build
bun run deploy
bun run preview
```

`bun run deploy` builds the Astro site and uploads `dist/` to the existing Cloudflare Pages project `jig-sh` through Wrangler on the production branch (`master`). Use `bun run deploy:preview` for a branch preview deployment. From the repo root, `scripts/deploy-landing.sh` runs the same production deployment with a frozen Bun install first; `scripts/deploy-landing.sh preview` deploys a preview.

## Structure

- `src/config/site.ts` — repo URLs and shared metadata
- `src/layouts/BaseLayout.astro` — document shell, fonts, global CSS, client script
- `src/components/` — page sections (hero, contract, vault, FAQ, etc.)
- `src/styles/global.css` — full landing page stylesheet
- `src/scripts/landing.ts` — crosshair, scroll reveal, terminal demo
- `wrangler.toml` — Cloudflare Pages project name and output directory

Build output goes to `landing/dist/`.
