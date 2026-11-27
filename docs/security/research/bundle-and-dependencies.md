# Shipped bundle and dependency review

Evidence for [#68](https://github.com/DylanAlmond/lockr/issues/68) (parent [#63](https://github.com/DylanAlmond/lockr/issues/63)).
Records what the 1.2.0 tree actually emits and resolves. It proposes no fixes.

Run on 2026-11-27, Windows, Node 24.19.0, `vite` 6.4.3, `vue-tsc` clean.

## 0. How the build was run, and why it matters

`yarn build` **cannot run at all** on the current tree:

```
$ yarn --version
! Corepack is about to download https://registry.yarnpkg.com/yarn/-/yarn-1.18.1.tgz
Error: Server answered with HTTP 404 ... yarn/-/yarn-1.18.1.tgz
```

`package.json` declares `"packageManager": "yarn@1.18.1"`, and no such version exists on the
registry (Yarn 1.x stops at 1.22.x). A fresh clone therefore cannot install or build with the
declared package manager. This is the single most consequential fact in this document: it is a
reproducibility defect, not a cosmetic one.

For the audit, the type gate and the production build were invoked directly:

```
node node_modules/vue-tsc/bin/vue-tsc.js --noEmit     # clean, no output
node node_modules/vite/bin/vite.js build              # 1906 modules, built in 10.43s
```

Dependency audit used a valid Yarn without touching the repository:

```
npx -y @yarnpkg/cli-dist@4.18.1 npm audit --all
```

## 1. Bundle contents

`dist/` after a clean production build:

| File | Size |
|---|---|
| `index.html` | 490 B |
| `assets/index-*.css` | 58.93 kB (7.55 kB gzip) |
| `assets/index-*.js` | 227.52 kB (72.03 kB gzip) |
| `assets/CalSansUI_*.woff2` | 62.47 kB |

Checks, with the search run against every emitted file:

| Check | Result |
|---|---|
| Source maps (`*.map`, `sourceMappingURL`) | **0** — none emitted, none referenced |
| `VITE_LOGO_API_TOKEN` or its value | **0 hits** — the dormant `fetchBrandLogoAsBase64` is tree-shaken out of the bundle |
| `logo.dev` / `img.logo` | **0 hits** |
| Seed fixtures (`Dev seed data`, `test12`, `src/seed.ts`) | **0 hits** — `seed.ts` is unimported and dropped |
| `eval(` / `new Function` | **0 hits** |
| External hosts | `http://www.w3.org` (SVG namespace) and `https://vuejs.org` (Vue's own error-text URL) — nothing else |
| CSS external references | none; the only `url()` is the bundled font |

Two hygiene defects in the emitted `index.html`:

- it still links `<link rel="icon" href="/vite.svg">`, but **no `vite.svg` is emitted** — a 404 on every launch;
- the document title is still `Tauri + Vue + Typescript App`.

`index.html` carries no CSP `<meta>`: expected, because Tauri injects the policy from
`tauri.conf.json` at runtime — which is `null` today.

## 2. Dependency facts

From `yarn.lock` (metadata version 10) and `node_modules`:

| Fact | Value |
|---|---|
| Resolved packages | 388 |
| Git, `https`, `file`, `link`, `portal` resolutions | 0 |
| Non-default registries | 0 |
| Workspace resolutions | 1 (`lockr@workspace:.`) |
| `vue-router` | resolves to `5.1.0` from `^5.1.0` |
| Packages with install scripts | `esbuild` (`postinstall`) |
| Install scripts enabled | **yes** — `.yarnrc.yml` sets `enableScripts: true` |

`esbuild`'s postinstall is its normal platform-binary install. Enabling scripts globally is the
notable part: any future dependency may run one during install.

## 3. Dependency audit

`yarn npm audit --all` against the committed lockfile:

| Severity | Count |
|---|---|
| High | 1 |
| Moderate | 3 |
| Low | 1 |

All five advisories are against **`vite` 6.4.3**, a devDependency — none is shipped to the User.
They split into two families:

- `launch-editor` (high, moderate, low variants): Windows `%`-escaping and editor-argument
  handling that can lead to arbitrary command execution for some editors;
- Vite dev-server issues (moderate, low): cross-origin script execution through custom
  middleware, and files outside `server.fs.allow` being served on non-Windows paths.

Reachability is bounded: these affect `tauri dev` / `vite` dev-server use on the developer's
machine, not the release bundle. That lowers severity; it does not remove the need to upgrade.

## 4. Release-mode observations

- `src-tauri/Cargo.toml` enables `tauri` features `["macos-private-api"]` only — **no `devtools`
  feature**, so a release build exposes no inspector. Tauri's default is the same.
- `dist/` is gitignored and `dist` holds no `.map` files, so the release payload is not a
  source-map disclosure risk.
- A full `tauri build` and an inspection of the installer payload were **not** run here; code
  signing and installer contents remain unverified (threat model AS5 / control T-15).

## 5. What this means for the audit

Positives to record:

- **No build-time secret reaches the bundle.** The logo.dev token is absent because the function
  is dead code; that also means re-enabling the feature would require a deliberate change.
- No source maps, no `eval`, no fixture data, no non-registry dependency resolutions.

Findings this evidence supports:

| Ref | Tier | Finding |
|---|---|---|
| F-B1 | P3 | `packageManager: yarn@1.18.1` does not exist; a fresh clone cannot install or build |
| F-B2 | P3 | `.yarnrc.yml` enables install scripts globally with no allowlist |
| F-B3 | P2 (dev) / P3 (shipped) | `vite` 6.4.3 carries 5 advisories (1 high, 3 moderate, 1 low); fixed by upgrading |
| F-B4 | P3 | `index.html` references a missing `/vite.svg` and keeps a scaffold title |
