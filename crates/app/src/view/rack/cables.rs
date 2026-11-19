//! The patch bay's Cables, kept from frame to frame so they can swing, and painted as sagging patch leads with a plug
//! at each end. The hanging is Corkboard's Strings (`patchbay_core::strand`).

use std::collections::HashMap;
use std::time::Instant;

use gpui_kit::*;
use patchbay_core::strand::{self, Point as Spot, Strand};

/// A Cable to hang: whose it is (`client/server`), its colour, and the jacks it's plugged into.
#[derive(Debug, Clone, PartialEq)]
pub struct Lead {
    pub key: String,
    pub color: Hsla,
    pub from: Spot,
    pub to: Spot,
}

#[derive(Default)]
pub struct Cables {
    strands: HashMap<String, (Hsla, Strand)>,
    /// When the last frame was, while any Cable is moving.
    clock: Option<Instant>,
    /// Time not yet stepped, so steps stay the same length whatever the frame rate.
    owed: f32,
}

/// How the rack's Cables move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Swing,
    /// Straight to where they'd come to rest, for Reduce motion and for captures.
    Still,
}

impl Cables {
    /// Plugs the Cables into `leads`' jacks and moves them on by the time since the last frame. True while any is
    /// still moving, so the caller asks for another frame.
    pub fn update(&mut self, leads: &[Lead], motion: Motion) -> bool {
        self.strands
            .retain(|key, _| leads.iter().any(|lead| &lead.key == key));
        for lead in leads {
            let (_, strand) = self
                .strands
                .entry(lead.key.clone())
                .and_modify(|(color, strand)| {
                    *color = lead.color;
                    strand.pin(lead.from, lead.to);
                })
                .or_insert_with(|| (lead.color, Strand::new(lead.from, lead.to)));
            if motion == Motion::Still {
                strand.settle();
            }
        }
        let now = Instant::now();
        let since = self
            .clock
            .map_or(0., |clock| (now - clock).as_secs_f32())
            // A long pause (a hidden window, a breakpoint) mustn't come back as a jolt.
            .min(0.1);
        self.owed += since;
        let steps = (self.owed / strand::STEP) as usize;
        self.owed -= steps as f32 * strand::STEP;
        for (_, strand) in self
            .strands
            .values_mut()
            .filter(|(_, strand)| !strand.is_still())
        {
            for _ in 0..steps {
                strand.step();
            }
        }
        let moving = self.strands.values().any(|(_, strand)| !strand.is_still());
        if moving {
            self.clock = Some(now);
        } else {
            self.clock = None;
            self.owed = 0.;
        }
        moving
    }

    /// Each Cable's colour and points, in a steady order so the same Cable is always drawn over the same others.
    pub fn shapes(&self) -> Vec<(Hsla, Vec<Spot>)> {
        let mut keys: Vec<&String> = self.strands.keys().collect();
        keys.sort();
        keys.into_iter()
            .filter_map(|key| self.strands.get(key))
            .map(|(color, strand)| (*color, strand.points().to_vec()))
            .collect()
    }
}

/// Paints each Cable in `shapes` as a smooth curve through its points: a soft shadow on the rack below it, the lead
/// in its colour, and a thin shine along its top so it reads as round. Then a plug over each end.
pub fn paint(
    bounds: Bounds<Pixels>,
    shapes: &[(Hsla, Vec<Spot>)],
    shadow: Hsla,
    width: f32,
    window: &mut Window,
) {
    let at = |spot: Spot, dx: f32, dy: f32| bounds.origin + point(px(spot.x + dx), px(spot.y + dy));
    let stroke =
        |shape: &[Spot], dx: f32, dy: f32, width: f32, color: Hsla, window: &mut Window| {
            let Some((first, rest)) = shape.split_first() else {
                return;
            };
            let mut path = PathBuilder::stroke(px(width));
            path.move_to(at(*first, dx, dy));
            // Through the middle of each piece, bending at each point, so the curve has no corners.
            for pair in rest.windows(2) {
                let middle = Spot::new((pair[0].x + pair[1].x) / 2., (pair[0].y + pair[1].y) / 2.);
                path.curve_to(at(middle, dx, dy), at(pair[0], dx, dy));
            }
            if let Some(last) = rest.last() {
                path.line_to(at(*last, dx, dy));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        };
    for (_, shape) in shapes {
        stroke(shape, width * 0.5, width * 1.4, width * 1.2, shadow, window);
    }
    for (color, shape) in shapes {
        stroke(shape, 0., 0., width, darker(*color, 0.25), window);
        stroke(shape, 0., 0., width * 0.7, *color, window);
        stroke(
            shape,
            0.,
            -width * 0.18,
            width * 0.22,
            shine(*color),
            window,
        );
    }
    for (color, shape) in shapes {
        for end in [shape.first(), shape.last()].into_iter().flatten() {
            plug(at(*end, 0., 0.), *color, width, window);
        }
    }
}

/// A plug pushed into a jack: a boot in the Cable's colour around a metal sleeve.
fn plug(centre: Point<Pixels>, color: Hsla, width: f32, window: &mut Window) {
    let disc = |radius: f32, color: Hsla, dy: f32, window: &mut Window| {
        let origin = centre + point(px(-radius), px(-radius + dy));
        window.paint_quad(
            fill(
                Bounds::new(origin, size(px(radius * 2.), px(radius * 2.))),
                color,
            )
            .corner_radii(px(radius)),
        );
    };
    disc(width * 1.9, hsla(0., 0., 0., 0.35), width * 0.5, window);
    disc(width * 1.8, darker(color, 0.3), 0., window);
    disc(width * 1.55, color, -width * 0.1, window);
    disc(width * 0.9, hsla(0., 0., 0.78, 1.), 0., window);
    disc(width * 0.5, hsla(0., 0., 0.45, 1.), 0., window);
}

fn darker(color: Hsla, by: f32) -> Hsla {
    Hsla {
        l: color.l * (1. - by),
        ..color
    }
}

fn shine(color: Hsla) -> Hsla {
    Hsla {
        l: (color.l + 0.3).min(0.95),
        a: 0.55,
        ..color
    }
}

#[cfg(test)]
mod tests {
    use super::{Cables, Lead, Motion};
    use gpui_kit::hsla;
    use patchbay_core::strand::Point;

    fn lead(key: &str) -> Lead {
        Lead {
            key: key.to_string(),
            color: hsla(0., 0.8, 0.5, 1.),
            from: Point::new(0., 0.),
            to: Point::new(200., 40.),
        }
    }

    #[test]
    fn a_cable_plugged_in_still_hangs_at_rest_at_once() {
        let mut cables = Cables::default();
        assert!(!cables.update(&[lead("pi/linear")], Motion::Still));
        assert_eq!(cables.shapes().len(), 1);
    }

    #[test]
    fn a_cable_plugged_in_swinging_falls_into_its_sag_over_frames() {
        let mut cables = Cables::default();
        assert!(cables.update(&[lead("pi/linear")], Motion::Swing));
    }

    #[test]
    fn a_cable_no_longer_wanted_is_unplugged() {
        let mut cables = Cables::default();
        cables.update(&[lead("pi/linear"), lead("pi/github")], Motion::Still);
        cables.update(&[lead("pi/github")], Motion::Still);
        assert_eq!(cables.shapes().len(), 1);
    }
}
