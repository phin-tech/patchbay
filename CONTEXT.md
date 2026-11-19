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

**Subscription**: a team's layer kept from its git repository, at the commit last accepted, as `patchbay.lock`
records it, or written from the MCP servers a LiteLLM proxy lists for the person's key, at the version last accepted.
A pasted team has none.

**Pending**: a team's manifest fetched but not yet accepted, with what it changes. Nothing pending reaches the
gateway.

**Offered**: a team's server it writes `enabled = false` for, or a skill in its sources it doesn't `install`: listed
for everyone, on for those who turn it on.

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

**Patch bay**: the main window's rack, a module for each client on the left and each server on the right, where a
client is given servers by patching Cables. Settings can turn it off.

**Cable**: a lead on the patch bay from a client's jack to a server's, one for each server the client is given. It
says what the client gets, not whether it's connected; that's the client's power switch.
_Avoid_: wire, link, connection

**Profile**: a set of servers exposed together at `/mcp/<profile>`; `default` is every enabled server.

**Skill**: a folder with a `SKILL.md` (frontmatter `name` and `description`) that a client reads as instructions.
_Avoid_: plugin, which is a Claude plugin that may hold skills

**Source**: where skills come from, written as `npx skills add` takes it: `owner/repo`, `owner/repo@skill`, a git URL
or a folder.

**Store**: `~/.agents/skills`, where every installed skill is kept, shared with the `skills` CLI; a client's skills
folder links to it.

**Lock**: `~/.agents/.skill-lock.json`, the `skills` CLI's record of each skill's source and hash, which Patchbay
writes too. A skill no lock records is the person's own.
