# Your team's servers and skills

A team keeps its servers and skills in a git repository, in a `patchbay.toml` at its top. Each person subscribes to
it, and Patchbay checks it for changes. Its servers run on your Mac with your own keys, and anything you add stays
yours.

![A team's changes, waiting to be looked at](media/team-changes.png)

## The team's repository

```toml
# patchbay.toml, at the top of git.example.com/acme/patchbay-team
[team]
name = "Acme"

# Secrets are only named. Each person gives their own, and Patchbay keeps it in their Keychain.
[secrets.github-token]
prompt = "A GitHub token with repo and read:org"
help = "https://github.com/settings/personal-access-tokens/new"

[servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github@2025.4.8"]   # pin versions
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "secret:github-token" }

# Offered but off: listed for everyone, and on only for those who turn it on.
[servers.notion]
url = "https://mcp.notion.com/mcp"
auth = "oauth"
enabled = false

[skills]
sources = [".", "vercel-labs/agent-skills"]   # "." is this repository, with skills in skills/<name>/SKILL.md
install = ["code-review", "web-design-guidelines"]
```

The team decides what's on and what's only offered:

- A server is on for everyone unless the team writes `enabled = false`, which offers it instead. Anyone can switch
  either kind on or off for themselves. The switch is saved in their own `patchbay.toml`, never the team's.
- The skills named in `install` are installed when you subscribe, or when an update adds them. Any other skill in
  the team's `sources` is only offered. Add it from **Skills ▸ Add Skills…** with that source.

To publish a change, push to the repository.

## Subscribing

Choose **Subscribe to a Team…** in the Servers menu and give the repository: a git URL
(`git@git.example.com:acme/patchbay-team.git`, `https://…`) or `owner/repo` on GitHub. A branch and a name of your
own for the team are optional. Press Return, and Patchbay fetches it and lists everything it adds: each server and
what it runs, each secret it asks for, and its skills. Nothing is kept until you choose **Subscribe**.

The team's secrets are yours. Patchbay asks for each one when its server first starts, and keeps it under the team's
name in your Keychain (`acme/github-token`). Your keys never go to the team's repository, and the team never sees
them.

A private repository on GitHub works once you've signed in with the GitHub CLI (`gh auth login`): Patchbay asks
`gh` for your sign-in whenever git needs one. Patchbay's git never shows a prompt. Anywhere else, use an SSH URL with a
key your ssh-agent holds, or clone the repository once in a terminal so git's credential helper saves your sign-in.
The same goes for skills from a private repository.

## Changes

Patchbay checks each team when it opens and every hour after that. **Check Teams for Changes** checks at once. A team
with changes is listed above your servers. **Review** shows what changed: servers added, removed or changed, field by
field, with the old and new value of each. Nothing changes until you choose **Accept**, which takes in exactly the
commit you looked at. **Later** leaves it for now.

When a team drops a server, your switch for it goes too. A skill the team drops stays installed: remove it yourself
if you don't want it. A skill of your own with the same name as one of the team's is never replaced. Updates to the
team's skills come through **Check for Skill Updates**, like any other skill.

Removing a team (**Remove Team** on one of its servers) takes out its servers and its subscription. Its keys stay in
your Keychain, so subscribing again doesn't ask for them.

## From a terminal

```sh
patchbay teams add git@git.example.com:acme/patchbay-team.git   # shows what it adds
patchbay teams add git@git.example.com:acme/patchbay-team.git --accept <commit>   # keeps it
patchbay teams                                                  # every team, and the commit accepted
patchbay teams check                                            # which teams have changed
patchbay teams update acme                                      # shows what changed
patchbay teams update acme --accept <commit>                    # takes in that commit and its skills
```

Each command that shows changes ends with the command that accepts them, with the full commit. Commits from the
same branch made after that one wait for the next check.

`patchbay.lock`, in `~/Library/Application Support/tech.phin.patchbay`, records each team's repository and the commit
last accepted.
