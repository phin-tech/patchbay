//! Patching by hand. Pressing an empty jack and dragging runs a new Cable from it, to whichever module on the other
//! side it's let go over. Pressing a plugged jack picks that Cable up by its plug, leaving its other end where it is,
//! to move it to another module on the same side, or pull it out by letting go over nothing. A press let go without
//! moving is a click, which plugs or pulls out that jack's Cable.

use patchbay_core::clients::Client;
use patchbay_core::strand::Point;

/// Which column of the rack a jack or a module is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Client,
    Server,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Client => Side::Server,
            Side::Server => Side::Client,
        }
    }
}

/// A Cable's two ends, by the client and the server it joins.
pub type Pair = (Client, String);

#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    /// The jack pressed, by the Cable it would hold, and its side.
    pub pressed: Pair,
    pub side: Side,
    /// The Cable picked up by its plug, if the jack held one.
    pub held: Option<Pair>,
    /// Where the press was and where the pointer is now, on the rack.
    pub start: Point,
    pub at: Point,
    pub moved: bool,
}

/// How far, in points, the pointer moves before a press is a drag, so a shaky click is still a click.
const SLOP: f32 = 4.;

impl Drag {
    pub fn new(pressed: Pair, side: Side, plugged: bool, at: Point) -> Drag {
        Drag {
            held: plugged.then(|| pressed.clone()),
            pressed,
            side,
            start: at,
            at,
            moved: false,
        }
    }

    pub fn move_to(&mut self, at: Point) {
        self.at = at;
        if (at.x - self.start.x).hypot(at.y - self.start.y) > SLOP {
            self.moved = true;
        }
    }

    /// The side whose end stays put: the pressed jack's for a new Cable, the far end's for one picked up.
    pub fn fixed(&self) -> Side {
        match self.held {
            Some(_) => self.side.other(),
            None => self.side,
        }
    }

    /// The side whose modules the loose end can go to.
    pub fn target(&self) -> Side {
        self.fixed().other()
    }

    /// The Cable the loose end would make over a module of [`Drag::target`]'s side: that module's client or server
    /// with the fixed end's.
    pub fn over(&self, client: Option<Client>, server: Option<&str>) -> Option<Pair> {
        match self.target() {
            Side::Server => Some((self.pressed.0, server?.to_string())),
            Side::Client => Some((client?, self.pressed.1.clone())),
        }
    }

    /// What letting go does, over the Cable `over` would make (if over a module), given which are plugged in.
    pub fn outcome(&self, over: Option<Pair>, plugged: impl Fn(&Pair) -> bool) -> Outcome {
        if !self.moved {
            return Outcome::Toggle(self.pressed.clone());
        }
        match (over, &self.held) {
            (Some(over), Some(held)) if &over == held => Outcome::Nothing,
            // Onto a Cable that's there already: the one held goes back where it was.
            (Some(over), _) if plugged(&over) => Outcome::Nothing,
            (Some(over), Some(held)) => Outcome::Move {
                from: held.clone(),
                to: over,
            },
            (Some(over), None) => Outcome::Plug(over),
            (None, Some(held)) => Outcome::Unplug(held.clone()),
            (None, None) => Outcome::Nothing,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Nothing,
    /// A click: plug the jack's Cable in, or pull it out.
    Toggle(Pair),
    Plug(Pair),
    Unplug(Pair),
    Move {
        from: Pair,
        to: Pair,
    },
}

#[cfg(test)]
mod tests {
    use super::{Drag, Outcome, Side};
    use patchbay_core::clients::Client;
    use patchbay_core::strand::Point;

    fn pair(client: Client, server: &str) -> (Client, String) {
        (client, server.to_string())
    }

    fn dragged(mut drag: Drag) -> Drag {
        drag.move_to(Point::new(100., 100.));
        drag
    }

    #[test]
    fn a_press_let_go_where_it_was_is_a_click_on_that_jack() {
        let drag = Drag::new(
            pair(Client::Pi, "linear"),
            Side::Server,
            false,
            Point::new(0., 0.),
        );
        assert_eq!(
            drag.outcome(None, |_| false),
            Outcome::Toggle(pair(Client::Pi, "linear"))
        );
    }

    #[test]
    fn dragging_from_an_empty_server_jack_to_a_client_plugs_that_server_into_that_client() {
        let drag = dragged(Drag::new(
            pair(Client::Pi, "linear"),
            Side::Server,
            false,
            Point::default(),
        ));
        assert_eq!(drag.target(), Side::Client);
        let over = drag.over(Some(Client::Cursor), None);
        assert_eq!(over, Some(pair(Client::Cursor, "linear")));
        assert_eq!(
            drag.outcome(over, |_| false),
            Outcome::Plug(pair(Client::Cursor, "linear"))
        );
    }

    #[test]
    fn a_plug_picked_up_moves_to_another_module_on_its_own_side() {
        let drag = dragged(Drag::new(
            pair(Client::Pi, "linear"),
            Side::Client,
            true,
            Point::default(),
        ));
        assert_eq!(drag.target(), Side::Client);
        let over = drag.over(Some(Client::Codex), None);
        assert_eq!(
            drag.outcome(over, |held| held == &pair(Client::Pi, "linear")),
            Outcome::Move {
                from: pair(Client::Pi, "linear"),
                to: pair(Client::Codex, "linear"),
            }
        );
    }

    #[test]
    fn a_plug_let_go_over_nothing_is_pulled_out() {
        let drag = dragged(Drag::new(
            pair(Client::Pi, "linear"),
            Side::Server,
            true,
            Point::default(),
        ));
        assert_eq!(
            drag.outcome(None, |_| true),
            Outcome::Unplug(pair(Client::Pi, "linear"))
        );
    }

    #[test]
    fn a_new_cable_let_go_onto_one_already_there_changes_nothing() {
        let drag = dragged(Drag::new(
            pair(Client::Pi, "linear"),
            Side::Client,
            false,
            Point::default(),
        ));
        let over = drag.over(None, Some("github"));
        assert_eq!(drag.outcome(over, |_| true), Outcome::Nothing);
    }
}
