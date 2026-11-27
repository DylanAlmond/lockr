# Renderer injection sink sweep

Research for #67 (parent #63). Factual inventory only; no fixes proposed.

**Scope.** `src/` (Vue 3 renderer). Two `src-tauri` config files are cited only to bound impact; Tauri internals, `node_modules`, and Rust string handling are outside the sweep except where cited.

**Origin labels.** **V** = vault/item-derived (a backend-returned string; may come from an imported, synced, or tampered vault file), **U** = direct user input, **S** = user-selected file, **C** = code constant, **—** = no data.

## Summary

| Sink class | Exists? | Non-constant data reaches it? |
|---|---|---|
| `v-html`, `innerHTML`, `outerHTML`, `document.write`, `insertAdjacentHTML`, `srcdoc`, `DOMParser`, `new Function`, `eval`, `compile` | **No** | — |
| Dynamic `href` / `action` / `formaction` | **No** | — |
| Dynamic `src` (`<img>`) | **Yes** | **V**, **U**, **S** |
| `postMessage` / `message` listener | **No** app-level | — |
| Tauri IPC (`invoke`) | **Yes** | **V** responses |
| `style` bindings | **Yes** | **V** (background colour only) |
| URL/query parsing → DOM/router param | Parsed; not written to DOM | **V**/`U` reaches params/filters only |
| `window.open`, `target="_blank"`, downloads | **No** | — |
| SVG loader | Static assets only | **No** |

**Headline.** Only two dangerous sink classes receive non-constant data: `<img :src>` (vault/profile icon strings, upload data URLs, picker files) and `background-color` (vault colour). Neither is a script-execution sink in WebView2 as written: `img` does not execute `javascript:` URLs or SVG script, and the colour value is assigned through the CSSOM, not concatenated into a stylesheet. No HTML, `href`, `postMessage`, or `window.open` sink exists in `src/`.

## 1. HTML / script injection — absent

Searches over `src/` for `v-html`, `innerHTML`, `outerHTML`, `document.write`, `insertAdjacentHTML`, `srcdoc`, `DOMParser`, `createContextualFragment`, `new Function`, `eval(`, `compile(`, and `setTimeout("…")` / `setInterval("…")` all return **0 matches**. There is no `template:` or `render:` option in `src/`; SFC templates are compiled at build time by `@vitejs/plugin-vue` (`vite.config.ts:10`). No template string is compiled at runtime.

The only `document.createElement` calls are `canvas` and `input` nodes (`src/util/imageUpload.ts:21,98,121,319`), never an element populated with markup.

All vault text is escaped into text nodes by mustache interpolation or made an input value: note body `NoteDetails.vue:187`, tags `TagList.vue:62`, field values `ItemField.vue:127`, modal title/message `AlertModal.vue:29,33`. Untrusted strings do reach attribute values (`:src`, `:id`, `:placeholder`, `:aria-label`), but Vue sets those through DOM APIs; quotes or angle brackets cannot break out into markup.

Dynamic components receive imported components, not vault strings: `Button.vue:48-50`, `Select.vue:116`, and `BaseModal.vue:51-52` (`modalState.component` set only by `openModal(markRaw(ImportedComponent))`, `useModal.ts:20-26`). `Dropdown.vue:258` uses `item.icon` typed `Component`; every call site passes a lucide import. The `v-bind` spreads (`Button.vue:35`, `Select.vue:97,101,142`, `Dropdown.vue:228`, `Titlebar.vue:35`, `BaseModal.vue:53`) are fed internal objects or literal call-site props; no spread key comes from vault content.

**No HTML parsing of untrusted data exists in `src/`.**

## 2. Dynamic URL attributes

**No `href` exists in `src/` at all** (`rg -n "href" src` → 0), and there are no literal `<a>` elements (`rg "<a\s" src` → 0). Navigation is entirely `RouterLink` with route objects (`Sidebar.vue:150,177,204`; `ItemList.vue:246`), which renders an anchor at runtime from the resolved route. `:href`, `:action`, and `:formaction` return 0. Router targets are name/param/query objects, so `javascript:` cannot be selected as a scheme; `itemRoute()` (`ItemList.vue:146-156`) spreads router params/query, which vue-router encodes, and redirect strings are constants (`router/index.ts:79-82`).

The only dynamic URL-bearing attribute is `img[src]`:

| Site | Value | Origin | Reachability |
|---|---|---|---|
| `ItemList.vue:248` | `item.icon` | **V** | Every list render; broadest exposure |
| `AccountDetails.vue:199` | `account.icon` | **V** | Account detail render |
| `CardDetails.vue:238` | `card.icon` | **V** | Card detail render |
| `IdentityDetails.vue:167` | `identity.icon` | **V** | Identity detail render |
| `NoteDetails.vue:168` | `note.icon` | **V** | Note detail render |
| `IconUpload.vue:58` | `modelValue` | **V**/`U` | Edit forms prefilled from vault; upload supplies a data URL |
| `Sidebar.vue:131` | `user.icon` | **V** | Sidebar render |
| `LoginView.vue:91` | `selectedUser.icon` | **V** | Login, after `?user=` selects a profile |
| `LoginView.vue:131` | `user.icon` | **V** | User picker render |

Internally, `img.src` is also assigned from `FileReader` output at `src/util/imageUpload.ts:57,161` (**S**, the file the user picked).

**Reachability and limits.** Icons are accepted with no URL/scheme/MIME validation: `normalize()` only drops empty strings (`src-tauri/src/vault_manager.rs:16-19`), and add/update/profile store the string as-is (`:459-463,531-533`; `user_manager.rs:188-189`). A tampered/synced vault or profile can therefore trigger an arbitrary `http(s)` request, or a `data:`/`file:` decode attempt — a network/beacon and decoder surface, not script execution. `javascript:` in `img.src` is inert; SVG via `<img>` is a non-scripted image document. No secret is interpolated into an icon URL.

Uploads are tighter: `accept="image/*"` (`imageUpload.ts:322`), an image MIME check (`:8,80`), decode via `Image`, canvas draw, then re-encode to PNG/WebP (`:48,152`). The stored value is a data URL with an image MIME, not attacker-authored SVG markup.

`fetchBrandLogoAsBase64` (`imageUpload.ts:263-310`) would fetch `https://img.logo.dev/name/${encodeURIComponent(brandName)}`; all call sites are commented out (`AccountEdit.vue:114-141`, `NewAccountForm.vue:8`). Dormant; no DOM sink.

**`javascript:` reachability: none.** No literal; no `href`/`action`/`formaction` sink; no anchor/iframe/object navigation path.

## 3. postMessage and Tauri IPC

`rg -n "postMessage|addEventListener\\(['\"]message|onmessage|event\\.origin" src` → **0 matches**. There is no message listener, so there is no app-level origin check to evaluate.

Existing listeners are same-document UI events: `click`, `keydown`, `resize`, `scroll` (`Dropdown.vue:213-216`; `BaseModal.vue:31`), plus `getCurrentWindow().onResized` (`Titlebar.vue:24`), whose callback ignores its payload. None receive cross-origin data.

Renderer-to-Rust traffic is **only** `invoke()` (`@tauri-apps/api/core`) in `useVault.ts:38-269` and `useUser.ts:15-114` (and the unimported dev `seed.ts:20-62`). There is no `@tauri-apps/api/event` `listen`, no `emit`, and no Tauri event carrying vault data into the DOM. `invoke<SafeItem[]>` generics are erased at runtime; response shape is trusted. A malformed response therefore reaches the sinks above as a string, not as executable code.

Tauri's own IPC uses a platform `postMessage` internally (WebView2 `chrome.webview.postMessage` / WKWebView `window.ipc.postMessage`) inside `@tauri-apps/api`; that is outside `src/` and gated by the capability ACL. Capabilities grant `core:default`, `opener:default`, and window controls, with no `fs`, `shell`, or `http` plugin (`src-tauri/capabilities/default.json:6-13`).

## 4. Dynamic CSS sinks

| Site | Value | Origin | Assessment |
|---|---|---|---|
| `Sidebar.vue:184` | `backgroundColor: vault.color` | **V** | Reachable on sidebar render |
| `AccountDetails.vue:156` | `parentVault?.color` (fallback C) | **V** | Reachable on detail render |
| `CardDetails.vue:197` | same | **V** | Reachable |
| `IdentityDetails.vue:126` | same | **V** | Reachable |
| `NoteDetails.vue:125` | same | **V** | Reachable |

The other three `:style` bindings receive constants or internal metrics: `ColorPicker.vue:42` uses `color.hex` from `VAULT_COLORS` (`constants.ts:3-12`); `Dropdown.vue:249` uses numbers from `getBoundingClientRect`/window metrics (`:34-86`); `Input.vue:60` forwards `attrs.style`, and no call site passes `style` (`AppLayout.vue:99-107` passes only `class`).

`vault.color` is an arbitrary backend string: `create_vault`/`update_vault` accept `color: String` without validation (`src-tauri/src/vault_manager.rs:137-156,343-361`; `commands.rs:209,286-293`), while the UI picker only offers fixed swatches (`ColorPicker.vue:35-45`). Edit/import/sync paths are not constrained by the picker.

Impact is bounded by how Vue applies object-valued `:style`: each entry is one declaration value via the CSSOM, not concatenated into `cssText`. Semicolons/braces do not create declarations, and an invalid colour (including `url(...)` for `background-color`) is discarded. This is a CSS **value** sink, not declarative injection; `background-color` cannot fetch a URL.

Tag strings are text only (`TagList.vue:62`); no tag-derived `:style`/`:class` exists. Icons use `img[src]`, never `background-image`/`url()`. `ctx.fillStyle = backgroundColor` (`imageUpload.ts:133`) is a canvas property set to a locally computed `rgb(r,g,b)` (`:248`), not DOM CSS. `document.body.style.overflow` writes constants (`BaseModal.vue:23,26,36`). `item.color` and `user.color` are never bound; only `vault.color` is displayed.

## 5. URL / query parsing

- `router/index.ts:31-34,50-51` maps `route.query.filter` to two equality booleans and forwards `route.query.tags` and `route.params.vaultId` into list props. `tags` is sent to the backend filter (`ItemList.vue:170-173` → `invoke("get_all_items")`), never rendered or used in a selector. `vaultId` reaches `get_vault_by_id` (`useVault.ts:56`), not the DOM.
- `route.params.itemId` reaches `setActiveItem` (`ItemPanel.vue:17,29-38` → `appStore.ts:34-39`); `route.params.mode` is only compared to `'edit'`/`'create'` (`ItemPanel.vue:20-21`). No parsed value is written as HTML/CSS.
- `route.query.user` is matched against profile ids (`LoginView.vue:71-81`); its only downstream effect is selecting a profile whose `icon` later reaches the `img` sink at `LoginView.vue:91`.
- `useSearch.ts:3-15` is bound to the search input (`AppLayout.vue:104`) and forwarded as `search_query` (`ItemList.vue:172`); it is displayed only as the input's value and never serialised into the URL.
- `useRouterHistory.ts:15` logs `window.history`; `:20-24` reads `history.state`, but `AppLayout.vue:27,84,94` consumes only the `canGoBack`/`canGoForward` booleans. No URL string from history reaches the DOM.
- There is no custom `URLSearchParams`/`decodeURIComponent` parsing and no `location` assignment. `router.beforeEach` redirects to constants (`router/index.ts:90-100`); no open-redirect or `javascript:` route target exists.

## 6. window.open, target="_blank", downloads

`rg -n "window\\.open|_blank|download\\s*=|createObjectURL|Blob\\(" src` → **0 matches**; `target=` has only two `event.target` locals (`Dropdown.vue:192`, `BaseModal.vue:16`). There are no anchors, so no reverse-tabnabbing surface. `window.location` / `location.href` assignment: 0 matches.

File access is a picker, not a download: `selectImageFile()` creates `input[type=file]`, sets `accept="image/*"`, and calls `input.click()` (`imageUpload.ts:317-335`). `canvas.toDataURL` (`:48,152`) produces strings for IPC; no anchor, `download` attribute, `createObjectURL`, or save dialog consumes them. Import/Export are disabled "Coming soon" (`SettingsView.vue:131-148`), so there is no vault download path.

`@tauri-apps/plugin-opener` is a dependency and `opener:default` is granted, but `rg "plugin-opener|openUrl|openPath" src` → 0. It is dormant, not a reachable sink today.

## 7. SVG handling

`vite-svg-loader` is configured (`vite.config.ts:3,10`) and only four literal, repo-controlled SVGs are imported as components: `Sidebar.vue:3` (`logo-text.svg`), `AuthView.vue:2-3` (`logo.svg`, `logo-shield.svg`), and `AboutAppModal.vue:3` (`logo.svg`). There is no dynamic `import()` of SVG and no path that feeds vault bytes to the loader or `<component :is>`; these are build-time constants.

Uploaded icons are never imported as components. `IconUpload` emits a base64 string (`IconUpload.vue:35-36`) rendered through `<img :src>` (`:58`); uploads are rasterised and re-encoded to PNG (`imageUpload.ts:48`). Vault/profile icons use `<img :src>` too. An uploaded or vault-supplied SVG is therefore an image document, not inline SVG; its scripts are disabled by the image-loading mode (browser-enforced).

## Evidence of absence

| Search (over `src/`) | Hits |
|---|---|
| `v-html`, `innerHTML`, `outerHTML`, `document.write`, `insertAdjacentHTML` | 0 |
| `srcdoc`, `DOMParser`, `createContextualFragment` | 0 |
| `new Function`, `eval(`, `compile(`, `template:` / `render:` | 0 |
| `postMessage`, `addEventListener("message")`, `onmessage` | 0 |
| `window.open`, `_blank`, `download=`, `createObjectURL`, `Blob(` | 0 |
| `href`, `:href`, `:action`, `:formaction`, `<a ` | 0 |
| `javascript:` | 0 |
| `:src=` | 9 (all `<img>`) |
| `:style=` | 8 (5 vault colour, 3 constant/internal) |
| `document.createElement` | 4 (`canvas` ×3, `input`) |
| `toDataURL` | 2 (PNG, WebP) |
| `fetch(` | 1 (dormant brand-logo fetch) |

## Flags and limits (not fixes)

1. **No CSP.** `src-tauri/tauri.conf.json:26-28` sets `"csp": null`. The absence of HTML/script sinks is currently the only barrier; any future `v-html`, markdown renderer, or inline-SVG path would be script-capable with full access to the IPC surface.
2. **Icons are an arbitrary-URL fetch.** `img[src]` accepts any backend string. It is a tracking/beacon and decoder surface, and becomes script-capable if a future change renders icons through CSS `url()`, `<object>`, inline SVG, or markdown.
3. **The renderer trusts backend types at runtime.** `invoke<T>` is an unchecked assertion; the only backend normalisation of `icon`/`color` is empty-string removal (`vault_manager.rs:16-19`). No renderer-side allowlist exists.
4. **`opener:default` is granted but unused.** Not a sink today, but directly available to any future renderer injection.
5. **No sync implementation in `src/`.** Peer values would enter through the same vault/profile JSON and reach the same `img`/colour sinks; ADR 0002 defines the peer model, not a separate renderer path.

## Open questions

- Is `vault.color` intended to be limited to `VAULT_COLORS`, making any other value a malformed-file case, or is it free-form?
- Should the renderer enforce an icon allowlist (for example `data:image/(png|webp);base64,`), or is backend validation the intended boundary?
- Does #63's threat model treat remote `img` fetches as an exfiltration/tracking channel worth constraining, or only script execution?
