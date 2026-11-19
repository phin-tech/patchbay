# Patchbay design

Patchbay is a Mac app, in Rust on GPUI and Typecase, that does two jobs:

1. **An MCP gateway.** Every AI client (Claude Code, Claude Desktop, Codex, Cursor, VS Code, Gemini CLI, Zed, …)
   points at one local Patchbay endpoint. Patchbay runs or connects to the real servers behind it and injects their
   secrets from the Keychain. It signs in to OAuth servers itself, namespaces their tools, and lets you choose which
   tools each client sees.
2. **A skills manager.** It installs, updates and removes agent skills (`<name>/SKILL.md` folders) from git sources
   into every agent's skills folder.

Both read one **manifest**. A team shares a manifest of its standard servers and skills. Each person subscribes to it,
fills in their own secrets when prompted, and adds their own servers on top.

The first slice is the gateway. Writing servers directly into each client's config ("sync") comes later, and only for
servers that hold no secrets.

## Shape

```
 Claude Code ─┐  http://127.0.0.1:7357/mcp/<profile>  (bearer token)
 Codex ───────┤
 Cursor ──────┼──▶ Patchbay.app (menu bar, opens at login) ──▶ stdio children (npx, uvx, binaries)
 VS Code ─────┤      │  gateway (rmcp)                     ──▶ remote MCP over streamable HTTP (+ OAuth)
 Claude Desk ─┘      │  Keychain (typecase-keychain)
   (stdio: `patchbay bridge <profile>`)
```

- **One signed binary.** `Patchbay.app/Contents/MacOS/patchbay` is the app, and with arguments it is the CLI
  (`patchbay bridge`, `patchbay status`, `patchbay skills …`), as gpu-template's `include_cli` does it.
- **The app owns the gateway and the Keychain.** Keychain access is tied to the code signature, so secrets are read
  by one long-lived process: the menu bar app, opened at login. Nothing else reads them, so a rebuild prompts once,
  not on every client launch.
- **`patchbay bridge`** is a thin stdio↔HTTP pipe for clients that only spawn commands. It holds no secrets. If the
  app isn't running it opens it (`open -g -b tech.phin.patchbay`) and waits for the port.
- **Localhost only, with a token.** The gateway binds `127.0.0.1` and needs a bearer token, which is written into
  each client's config. That token only reaches the gateway, never a downstream server. LAN access and remote access
  come later, opt-in.

## Crates

| Crate | What | UI-free |
| --- | --- | --- |
| `crates/core` | Manifest model, layering, secret references, client adapters, skills store and lock, settings | yes |
| `crates/gateway` | rmcp aggregator: downstream pool, namespacing, profiles, OAuth, HTTP server | yes |
| `crates/cli` | `bridge`, `status`, `import`, `skills`, `secrets` | yes |
| `crates/app` | GPUI: menu bar, main window, prompts, Settings | no |

Everything under `core`, `gateway` and `cli` is tested headless against fixtures and a sandboxed home
(`PATCHBAY_HOME`). Tests never touch the real `~/.claude*`, `~/.codex`, Cursor config or skills folders.

## The manifest

TOML, `patchbay.toml`. Values are never secrets; a secret is a reference, `secret:<name>`, declared once so Patchbay
knows how to ask for it.

```toml
# A team manifest, e.g. git.phin.tech/phin-tech/patchbay-team: patchbay.toml
[team]
name = "Phin Tech"

[secrets.github-token]
prompt = "A GitHub token with repo and read:org"
help = "https://github.com/settings/personal-access-tokens/new"

[servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github@2025.4.8"]   # pin versions in a shared manifest
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "secret:github-token" }

[servers.sentry]
url = "https://mcp.sentry.dev/mcp"
auth = "oauth"                       # Patchbay signs in; clients never see the token

[servers.linear]
url = "https://mcp.linear.app/mcp"
headers = { Authorization = "Bearer secret:linear-key" }
tools.deny = ["delete_*"]            # hidden from every client

[profiles.default]                   # what /mcp/default exposes
servers = ["github", "sentry", "linear"]

[skills]
sources = ["https://github.com/phin-tech/skills"]
install = ["pi-cold-review", "pi-handoff"]
```

- **Transport** is implied: `command` is stdio, `url` is streamable HTTP. Old SSE servers get `transport = "sse"`.
- **Secrets** are referenced in `env`, `headers` and `args` (`secret:x`, or `${secret:x}` inside a longer string).
- **Layers**, lowest first: each subscribed team manifest, then your own
  (`~/.config/patchbay/patchbay.toml`). Yours can add servers, change a team server's fields,
  turn one off (`[servers.github] enabled = false`), and define profiles.
- **Secret scope.** A team secret named `github-token` is stored per team (`<team>/github-token`), so two teams with
  the same name don't collide. Your own are `me/<name>`.

### Asking for secrets

When a server starts and a secret it references isn't in the Keychain, the server's status is **Needs a secret**. The
app brings forward a sheet with the secret's `prompt` and a `help` link, and a secure field. Saving it stores it and
starts the server. The menu bar shows a dot while anything is waiting. A client calling that server's tools in the
meantime gets an error it can show: "Patchbay needs a GitHub token. Open Patchbay to add it."

`patchbay secrets set <name>` does the same from a terminal, reading from stdin.

### OAuth

For a remote server with `auth = "oauth"` (or one that answers 401 with MCP's protected-resource metadata), Patchbay
is the OAuth client, following MCP's authorization spec: it discovers the authorization server, registers itself
dynamically if allowed (or uses `oauth.client_id` from the manifest), and runs authorization code + PKCE in the
browser. The redirect goes to a loopback URL (`http://127.0.0.1:<port>/oauth/callback`). Access and refresh tokens go
in the Keychain, refresh happens before expiry, and a revoked token puts the server back to **Sign in needed**,
asked for the way a missing secret is. rmcp's `auth` feature carries most of this. The spike in milestone 3 confirms
how much.

### Sharing with a team

- **Subscribe** with a git URL (Forgejo or GitHub) or a `patchbay://subscribe?url=…` link. Patchbay clones it into
  Application Support and records the commit in `patchbay.lock`.
- **A manifest runs commands on your Mac**, so nothing new runs unreviewed. Subscribing, and every update after it,
  shows what changed: servers added or removed, commands, args, URLs, which secrets are wanted. You accept it, and
  only then does the lock move. Updates are checked in the background, and the app says "Phin Tech has 2 changes to
  review".
- **Publishing** is a git push to the team repo. `patchbay export <server>` writes a server from your layer as team
  TOML with its secrets as references, ready to paste.

## The gateway

- **Downstream pool.** Stdio children start on first use (or at launch, per server), restart with backoff, and stop
  when idle. Remote servers keep one session each. Every call has a timeout.
- **Namespacing.** Tools, prompts and resources are exposed as `<server>__<name>` (`github__create_issue`), with the
  server's own description kept. A team can set `namespace = "gh"`. Two accounts of one server are two entries
  (`github-work`, `github-personal`).
- **Profiles.** `/mcp/<profile>` exposes one set of servers and tools, so Cursor can get fewer than Claude Code. Each
  client is pointed at a profile, `default` unless you change it.
- **Tool switches.** Allow and deny by glob in the manifest, and per tool in the app (stored in your layer). Read and
  write hints come from MCP's tool annotations and show in the app.
- **Notifications.** A downstream `tools/list_changed` is passed upstream, so clients refresh.
- **Catalog cache** in SQLite: the last tool list per server, so listing is instant and survives a slow server.
- **Later:** LAN access with its own token; remote access.

## Endpoints and layouts

Every endpoint is a **view**: what it gathers (a profile, or one server) and how it shows it (every tool, or
compact). They're made when first asked for, so a server or profile added later has its endpoint at once.

| Path | Gathers | Tools |
| --- | --- | --- |
| `/mcp`, `/mcp/<profile>` | a profile's servers | each under its server's namespace (`linear__list_issues`) |
| `/mcp/compact`, `/mcp/<profile>/compact` | a profile's servers | `search_tools`, `describe_tool`, `call_tool` |
| `/mcp/server/<name>` | one server | under their own names |
| `/mcp/server/<name>/compact` | one server | the three, over that server's tools |

Each client has a **choice**, kept in `clients.toml`: a profile, and a layout.

- **One entry**: `patchbay`, at the profile's flat endpoint.
- **An entry for each server**: `patchbay-<server>`, each at its server's endpoint, so the client lists and toggles
  each one on its own. When the profile's servers change, the window rewrites the client's entries to match.
- **Compact**: `patchbay`, at the profile's compact endpoint, for a client with many servers.

Every entry whose name starts with `patchbay` is Patchbay's own, to replace or remove; nothing else is touched.
`patchbay connect <client> --layout per-server --profile issues` does the same from a terminal, and
`patchbay bridge <route>` takes the same paths (`server/linear`, `issues/compact`).

## Pointing clients at it

Each client is an adapter: where its config lives, its format, and how to write one entry. Patchbay writes **one**
entry, named `patchbay`, and records that it owns it. It never edits other entries. Every write keeps unknown keys,
backs the file up, and replaces it atomically.

| Client | Config (user level) | Key | Entry |
| --- | --- | --- | --- |
| Claude Code | `~/.claude.json` | `mcpServers` | `type: http`, url, header |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `mcpServers` | stdio `patchbay bridge` |
| Codex | `~/.codex/config.toml` | `[mcp_servers.patchbay]` | `url`, `http_headers`, read back by `codex mcp get` |
| Cursor (and `cursor-agent`) | `~/.cursor/mcp.json` | `mcpServers` | `url`, `headers`; `cursor-agent mcp list-tools` connects |
| VS Code | `~/Library/Application Support/Code/User/mcp.json` | `servers` | `type: http`, `url`, `headers`, as `code --add-mcp` writes it |
| Gemini CLI | `~/.gemini/settings.json` | `mcpServers` | stdio bridge, until its HTTP entry is checked |
| opencode | `~/.config/opencode/opencode.json` | `mcp` | `type: remote`, `url`, `headers`; `opencode mcp list` connects |
| pi | `~/.pi/agent/mcp.json` | `mcpServers` | url, header, as `pi mcp add --url` writes it |
| omp | `~/.omp/agent/mcp.json` | `mcpServers` | url, header |
| PiG (pi in Go) | `~/.pig/agent/mcp.json` (`$PIG_HOME/agent`) | `mcpServers` | url, header, as `pig mcp add --url` writes it |

A client gets HTTP once its own tool has written or connected to the entry Patchbay writes; until then, the bridge.
These are the files and keys present on this Mac on 2026-11-18. Each adapter's exact entry shape is checked
against the client's current docs when it's written, with a fixture test per client.

**Import.** The first time a client is connected, Patchbay offers to move the servers it already has into your layer,
with any literal tokens moved into the Keychain, and then to remove them from the client. Nothing is removed without
asking.

## Skills

Patchbay installs skills the way the `skills` CLI (skills.sh, `npx skills add -g`) installs them for every project, in
the same folders and the same lock, so the two manage one set of skills and `npx skills list`, `check` and `update`
see what Patchbay installed. (This replaces an earlier plan for a store and lock of Patchbay's own.)

- **Sources** are written as the CLI takes them: `owner/repo`, `owner/repo@skill`, `owner/repo/path`, GitHub and
  GitLab URLs (with `/tree/<ref>/<path>`), `github:`/`gitlab:`, a `#ref@skill` fragment, any other git URL
  (git.phin.tech, SSH, `file://`), or a folder. A whole `npx skills add … --skill … -a …` line can be pasted into the
  window or given to `patchbay skills add`. Repositories are searched by the CLI's rules: `SKILL.md` at the top,
  `skills/` and its `.curated`/`.experimental`/`.system`, clients' own skills folders, `skills.sh.json`'s paths and
  Claude plugin manifests, then any `SKILL.md` five deep. git runs with no prompts and a timeout.
- **The store** is `~/.agents/skills/<name>/`, named as the CLI names it from the frontmatter. Each skill is linked
  (relatively, as the CLI links) into the skills folder of each client that keeps its own: Claude Code
  `~/.claude/skills`, Cursor `~/.cursor/skills`, VS Code `~/.copilot/skills`, Gemini CLI `~/.gemini/skills`,
  opencode `~/.config/opencode/skills`. Codex and pi read the store itself. Claude Desktop, omp and PiG have no
  folder the CLI knows, so they get none.
- **The lock** is the CLI's `~/.agents/.skill-lock.json` (version 3, `XDG_STATE_HOME` moves it). Patchbay writes the
  fields the CLI writes and keeps every other key. A GitHub skill's hash is its folder's git tree hash, any other's
  the CLI's SHA-256 content hash, so either tool can check the other's. A lock Patchbay can't read is left alone
  rather than started again.
- **Updating** fetches each source once, compares hashes, and lists the files that changed before applying. Applying
  fetches again and puts in place only what was listed, by its hash: a skill whose source has moved on since is left
  for the next check.
- **Skills no lock records are left alone**: a folder or link of the person's own, in the store or a client's folder,
  is shown as "Your own" and never replaced or removed. Installing over one is refused, and a client whose folder
  already has one of that name is skipped and said so.
- **Not yet**: installing what a manifest's `[skills]` names, project (not global) installs, and the CLI's
  `well-known` and download sources.

## The app

Built with Typecase's parts and the shared look (`typecase::shell`, 0.29):

- **Menu bar extra**: the gateway's state, servers needing a secret or a sign-in, copy the endpoint, open the window.
- **Main window**, sidebar of `Patch Bay`, `Servers`, `Clients`, `Skills`, `Teams`:
  - Patch Bay: a rack of a module for each client and each server, and a Cable for each server a client is given,
    patched by dragging; skills hang below and are patched the same way. It opens on it unless Settings turns it off.
  - Servers: a list with status (running, idle, needs a secret, sign in needed, failing), and a detail view with
    its tools and their switches. The **filter bar** narrows it (`server:github is:write is:off`).
  - Clients: each detected client, its profile, Connect and Disconnect.
  - Skills: installed, available from sources, and updates, with the diff.
  - Teams: subscriptions, pending changes to review.
- **Secret and sign-in sheets** as above, using `typecase-keychain`.
- **Command palette** (⌘K), **toasts** ("Connected Cursor · Undo"), **Settings** (port, open at login, agents'
  skills folders), **Welcome** at first launch: detect clients, subscribe to a team, connect.
- **Driver** for shots and agent checks, as Typecase has it.

## Milestones

0. **Scaffold** from gpu-template, Typecase bumped to the latest tag, `just check` green.
1. **Core**: manifest parsing and layering, secret references, `typecase-keychain` behind a `Secrets` trait (an
   in-memory one for tests), client adapters reading and writing the `patchbay` entry against fixtures.
2. **Gateway**: rmcp aggregator over stdio and HTTP downstreams, namespacing, one profile, secret injection,
   **Needs a secret** state, `patchbay bridge`. Milestone done: Claude Code and Claude Desktop calling a GitHub
   server through it.
3. **OAuth**: spike rmcp's auth against a real server (Sentry or Linear), then sign-in, refresh, Keychain.
4. **Teams**: subscribe, lock, review-before-update, export.
5. **Skills**: sources, store, symlinks, lock, update with diff.
6. **The app properly**: tool switches, profiles per client, import, Welcome. Then the compact mode and remote
   access.

## Open

- Where the team manifest lives: a Forgejo repo on git.phin.tech is assumed.
- How teammates install Patchbay: the download page (apps.phin.tech) is the template's default.
