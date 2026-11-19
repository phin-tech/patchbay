# Patchbay

One place for your MCP servers and agent skills, shared with your team

The words this project uses. Code, messages and docs use these terms; each entry lists the words to avoid.

## Language

**Command**: something the app does, by name (`file.new`), which a menu item, a key or the palette runs.
_Avoid_: action, which is GPUI's word for the message that carries one

**Settings**: what a person has chosen in the Settings window, kept between runs.
_Avoid_: preferences, config

**Manifest**: a `patchbay.toml`, a team's or the person's own, naming servers, secrets, profiles and skills.
_Avoid_: config (that's a client's file)

**Layer**: one manifest and whose it is (a team's, or "me"). Layers stack, lowest first, into what the gateway runs.

**Server**: an MCP server a manifest names, run as a command or reached at a URL.
_Avoid_: downstream, upstream

**Gateway**: Patchbay's one local MCP endpoint in front of every server, with each tool under its server's namespace.
_Avoid_: proxy, hub

**Client**: an AI tool that talks MCP to the gateway: Claude Code, Codex, pi, omp, opencode, Cursor and the rest.
_Avoid_: agent, when the tool is meant

**Secret**: a value a server needs that's never written in a manifest, only named (`secret:github-token`) and kept in
the Keychain under its layer's account (`phin-tech/github-token`, `me/github-token`).
_Avoid_: credential, env var

**Sign-in**: an OAuth server's tokens, which Patchbay gets in the browser and keeps (`oauth/<server>`).

**Bridge**: `patchbay bridge`, the gateway over stdio for a client that only runs commands.

**Profile**: a set of servers exposed together at `/mcp/<profile>`; `default` is every enabled server.
