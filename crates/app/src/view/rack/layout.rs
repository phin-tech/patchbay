//! Where everything on the patch bay goes, worked out before any of it is drawn, so the Cables know where the jacks
//! are. A rack has an ear down each side, the clients' modules stacked on the left and the servers' on the right, with
//! the jacks on the edges that face each other, so the Cables hang across the gap between the two columns.
//!
//! Each client's module has a jack for every server, numbered as the servers are, and each server's module a jack for
//! every client shown, lettered as the clients are, so every jack holds one Cable at most.

use patchbay_core::strand::Point;

/// A rectangle on the rack, in points from its top left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A module in a column: where it is, and its jacks' centres, in the other column's order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Module {
    pub rect: Rect,
    /// The part with the label and the switches, on the outer side.
    pub face: Rect,
    pub jacks: Vec<Point>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub ears: [Rect; 2],
    pub clients: Vec<Module>,
    pub servers: Vec<Module>,
    /// Blank plates filling each column below its modules, as a rack with room to spare has.
    pub blanks: Vec<Rect>,
}

impl Rect {
    pub fn contains(&self, at: Point) -> bool {
        at.x >= self.x
            && at.x <= self.x + self.width
            && at.y >= self.y
            && at.y <= self.y + self.height
    }
}

impl Layout {
    /// The client's or the server's module under `at`, by its place in its column.
    pub fn module_at(&self, clients: bool, at: Point) -> Option<usize> {
        let column = if clients {
            &self.clients
        } else {
            &self.servers
        };
        column.iter().position(|module| module.rect.contains(at))
    }

    /// The two ends of the Cable between client `client` and server `server`.
    pub fn cable(&self, client: usize, server: usize) -> Option<(Point, Point)> {
        let from = *self.clients.get(client)?.jacks.get(server)?;
        let to = *self.servers.get(server)?.jacks.get(client)?;
        Some((from, to))
    }
}

/// An ear's width, in rems: where the rack's screws go.
const EAR: f32 = 1.6;
/// Space above and below the modules, and between two of them, in rems.
const MARGIN: f32 = 0.8;
const BETWEEN: f32 = 0.3;
/// The width of a module's face, the label and switches, in rems: a client's has its power and layout too.
const CLIENT_FACE: f32 = 16.;
const SERVER_FACE: f32 = 12.;
/// From one jack's centre to the next, in rems, room for the jack and its number above it.
const PITCH: f32 = 1.8;
/// The shortest a module is, in rems: room for a label and a row of switches.
const SHORTEST: f32 = 5.4;
/// The narrowest the gap the Cables hang across is, in rems.
const GAP: f32 = 4.;

/// The narrowest a rack is, in rems: two columns with a jack each, and the gap.
pub fn narrowest() -> f32 {
    2. * (EAR + PITCH + MARGIN) + CLIENT_FACE + SERVER_FACE + GAP
}

/// Lays out `clients` and `servers` modules on a rack `width` points wide and at least `height` tall, or the
/// narrowest one, with `rem` points to a rem.
pub fn layout(clients: usize, servers: usize, width: f32, height: f32, rem: f32) -> Layout {
    let width = width.max(narrowest() * rem);
    let ear = EAR * rem;
    let inner = width - 2. * ear;
    // A wide window gives the Cables more room to hang, and the jacks the rest, the same on both sides.
    let gap = (inner * 0.15).max(GAP * rem);
    let jack_room = ((inner - gap - (CLIENT_FACE + SERVER_FACE) * rem) / 2.).max(PITCH * rem);
    let pitch = PITCH * rem;
    // A row below another needs room for its numbers clear of the plugs above.
    let row_pitch = pitch * 1.3;
    let across = (((jack_room - MARGIN * rem) / pitch).floor() as usize).max(1);
    let stack = |count: usize, jacks: usize, left: f32, face: f32, faces_left: bool| {
        let column = face + jack_room;
        let rows = jacks.div_ceil(across).max(1);
        let height = (SHORTEST * rem).max(rows as f32 * row_pitch + 1.2 * rem);
        (0..count)
            .map(|at| {
                let y = MARGIN * rem + at as f32 * (height + BETWEEN * rem);
                let rect = Rect {
                    x: left,
                    y,
                    width: column,
                    height,
                };
                let face_rect = Rect {
                    x: if faces_left {
                        left
                    } else {
                        left + column - face
                    },
                    y,
                    width: face,
                    height,
                };
                // The jacks sit against the edge facing the gap, a client's on its right and a server's on its left,
                // so a Cable's ends are as near each other as they can be.
                let top = y + (height - rows as f32 * row_pitch) / 2. + row_pitch * 0.6;
                let jacks = (0..jacks)
                    .map(|jack| {
                        let (row, col) = (jack / across, jack % across);
                        let offset = MARGIN * rem + pitch * (col as f32 + 0.5);
                        // Numbered left to right either way, in a block against the gap.
                        let block = jacks.min(across) as f32 * pitch;
                        let x = if faces_left {
                            left + column - MARGIN * rem - block + pitch * (col as f32 + 0.5)
                        } else {
                            left + offset
                        };
                        Point::new(x, top + row as f32 * row_pitch)
                    })
                    .collect();
                Module {
                    rect,
                    face: face_rect,
                    jacks,
                }
            })
            .collect::<Vec<_>>()
    };
    let client_face = CLIENT_FACE * rem;
    let clients = stack(clients, servers, ear, client_face, true);
    let servers = stack(
        servers,
        clients.len(),
        ear + client_face + jack_room + gap,
        SERVER_FACE * rem,
        false,
    );
    let bottom = |modules: &[Module]| {
        modules.last().map_or(MARGIN * rem, |module| {
            module.rect.y + module.rect.height + BETWEEN * rem
        })
    };
    let height = (bottom(&clients).max(bottom(&servers)) + MARGIN * rem).max(height);
    let plate = SHORTEST * rem / 2.;
    // Each column's spare room, in plates half a module tall, to the foot of the rack.
    let blanks = [
        (ear, client_face + jack_room, bottom(&clients)),
        (
            ear + client_face + jack_room + gap,
            SERVER_FACE * rem + jack_room,
            bottom(&servers),
        ),
    ]
    .into_iter()
    .flat_map(|(x, width, top)| {
        let count = ((height - MARGIN * rem - top) / (plate + BETWEEN * rem))
            .floor()
            .max(0.) as usize;
        (0..count).map(move |at| Rect {
            x,
            y: top + at as f32 * (plate + BETWEEN * rem),
            width,
            height: plate,
        })
    })
    .collect();
    Layout {
        width,
        height,
        ears: [
            Rect {
                x: 0.,
                y: 0.,
                width: ear,
                height,
            },
            Rect {
                x: width - ear,
                y: 0.,
                width: ear,
                height,
            },
        ],
        clients,
        servers,
        blanks,
    }
}

#[cfg(test)]
mod tests {
    use super::{layout, narrowest};

    #[test]
    fn clients_hang_on_the_left_and_servers_on_the_right_with_a_gap_between() {
        let rack = layout(3, 2, 900., 0., 13.);
        let client = rack.clients[0].rect;
        let server = rack.servers[0].rect;
        assert!(client.x + client.width < server.x);
        assert!(rack.ears[0].width <= client.x);
        assert!(server.x + server.width <= rack.ears[1].x + 0.01);
    }

    #[test]
    fn each_client_has_a_jack_for_every_server_and_each_server_one_for_every_client() {
        let rack = layout(3, 4, 900., 0., 13.);
        assert!(rack.clients.iter().all(|module| module.jacks.len() == 4));
        assert!(rack.servers.iter().all(|module| module.jacks.len() == 3));
        assert!(rack.cable(2, 3).is_some());
        assert!(rack.cable(3, 0).is_none());
    }

    #[test]
    fn a_cable_runs_from_the_client_side_of_the_gap_to_the_server_side() {
        let rack = layout(2, 2, 900., 0., 13.);
        let (from, to) = rack.cable(1, 0).unwrap();
        assert!(from.x < to.x);
        let client = rack.clients[1].rect;
        assert!(from.y > client.y && from.y < client.y + client.height);
    }

    #[test]
    fn jacks_sit_inside_their_module_and_clear_of_its_face() {
        let rack = layout(2, 12, 700., 0., 13.);
        for module in rack.clients.iter().chain(&rack.servers) {
            for jack in &module.jacks {
                let (rect, face) = (module.rect, module.face);
                assert!(jack.x > rect.x && jack.x < rect.x + rect.width);
                assert!(jack.y > rect.y && jack.y < rect.y + rect.height);
                assert!(jack.x < face.x || jack.x > face.x + face.width);
            }
        }
    }

    #[test]
    fn more_servers_than_fit_in_a_row_make_the_clients_modules_taller() {
        let few = layout(1, 1, 700., 0., 13.);
        let many = layout(1, 20, 700., 0., 13.);
        assert!(many.clients[0].rect.height > few.clients[0].rect.height);
    }

    #[test]
    fn a_short_rack_reaches_the_foot_of_the_window_with_blank_plates_below_its_modules() {
        let rack = layout(1, 1, 900., 800., 13.);
        assert_eq!(rack.height, 800.);
        assert!(!rack.blanks.is_empty());
        assert!(
            rack.blanks
                .iter()
                .all(|blank| blank.y + blank.height <= rack.height)
        );
    }

    #[test]
    fn a_narrow_window_still_gets_the_narrowest_rack() {
        let rack = layout(1, 1, 100., 0., 13.);
        assert_eq!(rack.width, narrowest() * 13.);
    }
}
