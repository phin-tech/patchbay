# Your tools: clients and layouts

Patchbay calls the AI tools it serves **clients**. Each one finds its MCP servers in a config file of its own; **Connect**
writes Patchbay's entries there, and only those, keeping a copy of the file as it was beside it
(`<file>.patchbay-backup`). **Disconnect** takes them out again. Restart a tool after connecting it, or after changing
what it gets.

![Choosing what Codex gets](media/clients.png)

| Client | Its config | How it reaches Patchbay |
| --- | --- | --- |
| Claude Code | `~/.claude.json` | over HTTP, added with Claude Code's own `claude mcp` |
| Codex | `~/.codex/config.toml` | over HTTP |
| Cursor and `cursor-agent` | `~/.cursor/mcp.json` | over HTTP |
| VS Code | `~/Library/Application Support/Code/User/mcp.json` | over HTTP |
| opencode | `~/.config/opencode/opencode.json` | over HTTP |
| pi | `~/.pi/agent/mcp.json` | over HTTP |
| omp | `~/.omp/agent/mcp.json` | over HTTP |
| PiG | `~/.pig/agent/mcp.json` | over HTTP |
| Claude Desktop | `~/Library/Application Support/Claude/claude_desktop_config.json` | `patchbay bridge` |
| Gemini CLI | `~/.gemini/settings.json` | `patchbay bridge` |

Over HTTP, the tool talks to the gateway at `http://127.0.0.1:7357` with its token, so Patchbay has to be running: keep
it in the menu bar, or turn on Open at login in Settings. A tool given `patchbay bridge` runs it as a command instead,
and the bridge opens Patchbay in the background if it isn't running.

## What each tool gets

**Servers.** Every server, a profile's, or **Only these**, ticked one by one. A server you add later reaches a tool on
Every server at once, and one on Only these when you tick it.

**Layout.**

- **One entry**, `patchbay`, with each tool named after its server: Linear's `list_issues` is `linear__list_issues`.
- **An entry for each server**, `patchbay-linear`, `patchbay-github`, so the tool lists each one and turns it on and off
  by itself. Patchbay keeps them in step as your servers change.
- **Compact**: one entry with three tools, `search_tools`, `describe_tool` and `call_tool`, that find and run every
  other one. A tool with many servers keeps its context for the work.

From a terminal: `patchbay connect <client> --servers linear,github --layout per-server`.

## A server that isn't ready

A server waiting for its key or a sign-in shows up in your tools as `<server>__needs_setup`. Calling it says what to do
in Patchbay, so an agent can tell you rather than fail quietly.
