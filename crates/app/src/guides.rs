//! The tour: a few steps over the main window, each pointing at a part of it with a card saying what it's for. It's
//! Typecase's tour, as Red Pen's and Corkboard's are; what each step says and points at is here, and `view` marks the
//! places with `.tour_target(…)` and shows the place a step needs.
//!
//! Unlike theirs it shows your own servers and clients, not made-up ones, since every step goes on with Next and
//! takes no clicks: nothing in the tour changes a client's config or a skill.

use gpui_kit::base::Placement;
use rust_i18n::t;
use typecase::tour::Step;

use crate::shortcuts::key_for;
use crate::view::Place;

/// A Command's key, as a Mac shows it.
fn key(command: &str) -> String {
    key_for(command).unwrap_or_default()
}

fn step(title: std::borrow::Cow<str>, body: std::borrow::Cow<str>) -> Step {
    Step::new(title.into_owned(), body.into_owned())
}

/// The tour's steps, each with the place the window shows for it. Without the patch bay, which Settings can turn off,
/// its step is left out.
pub fn overview(patch_bay: bool) -> Vec<(Step, Place)> {
    let first = if patch_bay {
        Place::PatchBay
    } else {
        Place::Servers
    };
    let mut steps = vec![
        (
            step(
                t!("A Quick Look Around"),
                t!(
                    "Patchbay keeps your MCP servers in one place and gives them to every AI tool on this Mac \
                     through one gateway. Nothing here changes while the tour runs. Escape ends it, and → goes on."
                ),
            ),
            first,
        ),
        (
            step(
                t!("Places"),
                t!(
                    "%{first} to %{last} go between the Patch Bay, Servers, Clients and Skills. Each counts what it \
                     holds: the cables plugged in, the servers, the clients connected and the skills.",
                    first = key("view.patch-bay"),
                    last = key("view.skills")
                ),
            )
            .on("places")
            .placement(Placement::Right),
            first,
        ),
        (
            step(
                t!("The Patch Bay"),
                t!(
                    "Each client is a module, with a cable to each server and skill it's given. Drag a plug to \
                     another client to move it, or pull it out to take it away. The View menu hides the servers or \
                     the skills here."
                ),
            )
            .on("rack")
            .placement(Placement::Bottom),
            Place::PatchBay,
        ),
        (
            step(
                t!("Your Servers"),
                t!(
                    "Every server, with a switch to turn it off. %{add} adds one from its JSON, its command or its \
                     URL, and Import from Your Clients brings in the ones your tools have already.",
                    add = key("servers.add")
                ),
            )
            .on("list")
            .placement(Placement::Right),
            Place::Servers,
        ),
        (
            step(
                t!("The Server Chosen"),
                t!(
                    "What it runs, the keys it wants, which stay in your Keychain, its sign-in and its tools. The \
                     Servers menu does what its buttons do, so the palette can too."
                ),
            )
            .on("detail")
            .placement(Placement::Left),
            Place::Servers,
        ),
        (
            step(
                t!("Filter"),
                t!(
                    "%{filter} narrows the list. server:, team: and is: narrow it further: is:waiting shows the \
                     servers waiting for a key or a sign-in, and is:write the tools that change something.",
                    filter = key("view.filter")
                ),
            )
            .on("filter")
            .placement(Placement::Right),
            Place::Servers,
        ),
        (
            step(
                t!("Teams"),
                t!(
                    "Subscribe to a Team, in the Servers menu, keeps a team's servers and skills from a git \
                     repository. Patchbay checks it every hour, and nothing it changes reaches you until you've \
                     reviewed it."
                ),
            )
            .on("list")
            .placement(Placement::Right),
            Place::Servers,
        ),
        (
            step(
                t!("Clients"),
                t!(
                    "Each AI tool Patchbay knows. Connecting one gives it a single entry for the gateway, and its \
                     profile and layout choose which servers it sees and how."
                ),
            )
            .on("list")
            .placement(Placement::Right),
            Place::Clients,
        ),
        (
            step(
                t!("Skills"),
                t!(
                    "Skills are kept once, in ~/.agents/skills, and linked into each tool's own folder. Add Skills \
                     finds them in a repository, and Check for Skill Updates fetches what's new."
                ),
            )
            .on("list")
            .placement(Placement::Right),
            Place::Skills,
        ),
        (
            step(
                t!("The Gateway"),
                t!(
                    "Every client reaches your servers at this URL, which %{copy} copies. It keeps running with the \
                     window closed, from the menu bar.",
                    copy = key("gateway.copy-url")
                ),
            )
            .on("gateway")
            .placement(Placement::Right),
            first,
        ),
        (
            step(
                t!("Every Command"),
                t!(
                    "%{palette} finds any command by name, and %{keys} shows every key. That's the tour: it's in \
                     the Help menu whenever you want it again.",
                    palette = key("app.palette"),
                    keys = key("app.keys")
                ),
            ),
            first,
        ),
    ];
    if !patch_bay {
        steps.retain(|(_, place)| *place != Place::PatchBay);
    }
    steps
}

#[cfg(test)]
mod tests {
    use super::{key, overview};
    use crate::view::Place;

    #[test]
    fn every_key_the_tour_names_is_bound() {
        for command in [
            "view.patch-bay",
            "view.skills",
            "servers.add",
            "view.filter",
            "gateway.copy-url",
            "app.palette",
            "app.keys",
        ] {
            assert!(!key(command).is_empty(), "{command} has no key");
        }
    }

    #[test]
    fn without_the_patch_bay_the_tour_leaves_out_its_step_and_never_shows_it() {
        let with = overview(true);
        let without = overview(false);
        assert_eq!(without.len(), with.len() - 1);
        assert!(without.iter().all(|(_, place)| *place != Place::PatchBay));
    }
}
