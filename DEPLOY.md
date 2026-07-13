# Deploying fanva

fanva's UI runs **entirely in the browser** — the translator (agent loop, the
gerna/smuni/camxes gates) is compiled into a WASM bundle and there is **no fanva
server**. "Deploying" is therefore just hosting one static bundle, plus one optional
Cloudflare Worker (`fanva-proxy`) for the jbotci tool-use path.

There is one web deployable, plus the optional proxy (don't conflate them):

| URL | Crate / dir | What it is |
|-----|-------------|-----------|
| `dhilipsiva.dev/fanva` | `fanva-ui` (Dioxus app) | The English→Lojban translator playground |
| `fanva-proxy.<acct>.workers.dev/mcp` | `fanva-proxy` (Cloudflare Worker) | Optional CORS-adding reverse proxy to jbotci; off by default |

## 1. Ship the frontend

The build/host pipeline lives in the **external `dhilipsiva/dhilipsiva.dev` repo**,
not here. This repo only *pings* it: on every push to `main`,
[`.github/workflows/redeploy-site.yml`](.github/workflows/redeploy-site.yml) fires a
`repository_dispatch` (`event_type=fanva-updated`) that tells the site to rebuild and
re-pull this crate. (It self-skips, staying green, until the `SITE_DISPATCH_TOKEN`
secret exists in this repo.)

So **shipping the translator = merging your branch into `main`.**

- **fanva-ui is served under the `/fanva/` subpath, but it stays root-relative** — this
  repo commits **no `Dioxus.toml`/base_path**. The `/fanva/` base path is applied by the
  site repo at build time (same as nibli-ui at `/nibli-playground/` and voksa). Keep it
  that way: a committed `base_path` bakes the subpath into the wasm and breaks the local
  `just ui` dev server.
- **The site build MUST fetch `dictionary-en.json` before `dx build`** (the public
  lensisku dump `just fetch-dict` pulls, dropped at the checkout root). The dictionary is
  a compile-time input: with it, the bundle ships the full smuni-dictionary; without it
  the build silently falls back to the ~175 curated entries and the deployed translator
  loses long-tail vocabulary (a `cargo:warning` is emitted, not an error). The site's
  build script should carry this step (warn-and-continue on fetch failure — the fallback
  still builds).
- Everything the translator needs is **baked into the `fanva-ui` bundle at build time**:
  the `fanva` engine + `gerna`/`smuni` are **path dependencies compiled in** (no separate
  service), and the vendored **camxes** grammar ships as `asset!()` static assets
  (`fanva-ui/assets/js/vendor/camxes/…`, wired in `fanva-ui/src/main.rs`) — the official
  gate. The local gates (**gerna + smuni + camxes**) run in-browser with **zero network**.
  The *only* optional network calls are the user's own BYO-key LLM request and (if jbotci
  is enabled) the proxy below.

### Local release preview (optional)

To build the exact shipping bundle locally (a preview / pre-merge sanity check — the
**production** build runs in the external site repo):

```sh
just build-ui        # dx build --release
# output: target/dx/fanva-ui/release/web/public/  — serve it with any static server
```

Because there is no `base_path`, the local bundle is served from the **root** `/`; the
site repo re-homes it under `/fanva/`.

## 2. Optional: the jbotci proxy (`fanva-proxy/`)

jbotci dictionary/grammar/parser tool-use during drafting is **off by default** and
degrades cleanly to the local gates when the proxy URL is blank. The UI ships prefilled
with `https://fanva-proxy.dhilipsiva.workers.dev/mcp` (`fanva-ui/src/main.rs`), inert
until the user enables jbotci in the settings modal.

**For the `dhilipsiva.dev/fanva` origin, no proxy change is required.** CORS keys on the
*origin* (`https://dhilipsiva.dev`), not the path — and that origin is already the
`ALLOWED_ORIGINS` value in `fanva-proxy/wrangler.toml` and the code default in
`fanva-proxy/src/index.js`. A browser on `https://dhilipsiva.dev/fanva/` is already
allowed.

> **Shared worker — do not casually redeploy.** The deployed
> `fanva-proxy.dhilipsiva.workers.dev` worker currently **also serves nibli's UI** until
> nibli's Lojban purge lands. `ALLOWED_ORIGINS` is a build-time `[vars]` value, so a
> `wrangler deploy` from this repo **overwrites** the live allowlist. If it ever must
> change, **union** the origins (comma-separated) rather than replacing — dropping
> nibli's origin silently breaks nibli's jbotci calls (CORS block → local-gates
> degradation). Coordinate first. Full worker runbook: [`fanva-proxy/DEPLOY.md`](fanva-proxy/DEPLOY.md).

To stand up a *new* proxy (only if you are not using the existing shared one): deploy per
[`fanva-proxy/DEPLOY.md`](fanva-proxy/DEPLOY.md) (`npx wrangler login`, `npm run deploy`),
set `ALLOWED_ORIGINS` to your app origin **before** deploy, and paste the resulting
`https://fanva-proxy.<acct>.workers.dev/mcp` into the settings modal. Local dev origins
(`http://localhost:8080`) belong in `fanva-proxy/.dev.vars` (gitignored), never the
committed `wrangler.toml` var.

## 3. Acceptance ("done when")

Hosted translation works end-to-end at `https://dhilipsiva.dev/fanva/`:

- **Always** (no proxy, no jbotci): open the app, enter your LLM API key in settings
  (BYO-key, held in that tab's memory only), translate a sentence on the Source tab — the
  draft is validated by the local **gerna + smuni + camxes** gates and the self-correction
  trace shows the attempts + the three gate chips; a valid result fills the Lojban tab.
- **With the proxy** (jbotci on): the self-correction trace shows jbotci tool calls, and
  the Back-translation tab's **Deep meaning (tersmu)** view renders jbotci's semantic
  graph. No CORS errors in the browser console.
- **`window.camxes_validate` is defined** in the page (DevTools console). If it is
  missing, the camxes gate is **fail-open** — the app silently runs on **2 gates, not 3**.

Exercising the translator needs a user-supplied LLM key — there is no shared key and no
fanva server; the request goes straight from the browser to the chosen provider.
