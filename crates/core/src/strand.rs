//! A Cable on the patch bay: a chain of points between two jacks, a little longer than the gap between them so it
//! sags, moved by Verlet integration. Each step carries every point on by how far it moved last step, adds gravity,
//! then pulls neighbours back to their length a few times over, with the two ends held on their jacks. Moving a jack
//! sets the Cable swinging, and it comes to rest on its own.
//!
//! Coordinates are the rack's, in points, with y growing downward as on screen.

/// A place on the rack.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    fn lerp(self, other: Point, t: f32) -> Point {
        Point::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

/// How many points make a Cable. More hang more smoothly and cost more.
const POINTS: usize = 14;
/// How much longer a Cable is than the gap between its jacks.
const SLACK: f32 = 1.12;
/// The shortest a Cable is, so two jacks close together still have a loop between them.
const SHORTEST: f32 = 48.;
/// Downward, in points a second squared: a little less than a real cable's, so it reads as light.
const GRAVITY: f32 = 1400.;
/// How much of its speed a point keeps each step: less than all, so a swing dies away.
const KEEP: f32 = 0.97;
/// How many times a step pulls neighbours back to their length. More is stiffer.
const PASSES: usize = 16;
/// How far, in points, the furthest point may move in a step for the Cable to count as still.
const STILL: f32 = 0.02;
/// One step, in seconds. Steps are always this long, so a Cable behaves the same at any frame rate.
pub const STEP: f32 = 1. / 120.;

#[derive(Debug, Clone, PartialEq)]
pub struct Strand {
    now: Vec<Point>,
    before: Vec<Point>,
    /// How long each piece between two points is.
    piece: f32,
    /// How far the furthest point moved in the last step.
    moved: f32,
}

impl Strand {
    /// A Cable pinned between `from` and `to`, drawn straight at first, so it falls into its sag.
    pub fn new(from: Point, to: Point) -> Strand {
        let now: Vec<Point> = (0..POINTS)
            .map(|at| from.lerp(to, at as f32 / (POINTS - 1) as f32))
            .collect();
        Strand {
            before: now.clone(),
            now,
            piece: piece(from, to),
            moved: f32::INFINITY,
        }
    }

    /// A Cable between `from` and `to` as it hangs once still, for when nothing should move.
    pub fn at_rest(from: Point, to: Point) -> Strand {
        let mut strand = Strand::new(from, to);
        strand.settle();
        strand
    }

    /// Steps it until it's still, as it would come to rest if left alone.
    pub fn settle(&mut self) {
        // Enough steps for any Cable a window holds to settle, with a limit so it can never run on.
        for _ in 0..4000 {
            if self.is_still() {
                break;
            }
            self.step();
        }
    }

    /// Moves the ends to `from` and `to`, as when the modules they.re plugged into move. The Cable takes its length from
    /// the new gap, so a jack moving away doesn't pull it taut.
    pub fn pin(&mut self, from: Point, to: Point) {
        let (first, last) = (self.now[0], self.now[POINTS - 1]);
        if first == from && last == to {
            return;
        }
        self.now[0] = from;
        self.before[0] = from;
        self.now[POINTS - 1] = to;
        self.before[POINTS - 1] = to;
        self.piece = piece(from, to);
        self.moved = f32::INFINITY;
    }

    /// Moves the Cable on by one [`STEP`].
    pub fn step(&mut self) {
        let (from, to) = (self.now[0], self.now[POINTS - 1]);
        let fall = GRAVITY * STEP * STEP;
        let start = self.now.clone();
        for at in 1..POINTS - 1 {
            let (now, before) = (self.now[at], self.before[at]);
            self.now[at] = Point::new(
                now.x + (now.x - before.x) * KEEP,
                now.y + (now.y - before.y) * KEEP + fall,
            );
        }
        self.before = start;
        for _ in 0..PASSES {
            for at in 0..POINTS - 1 {
                let (a, b) = (self.now[at], self.now[at + 1]);
                let gap = a.distance(b);
                if gap <= f32::EPSILON {
                    continue;
                }
                // Each end of a piece moves half of what's needed, but an end on a jack doesn't move at all.
                let off = (gap - self.piece) / gap;
                let (dx, dy) = ((b.x - a.x) * off, (b.y - a.y) * off);
                let (share_a, share_b) = match (at == 0, at + 1 == POINTS - 1) {
                    (true, true) => (0., 0.),
                    (true, false) => (0., 1.),
                    (false, true) => (1., 0.),
                    (false, false) => (0.5, 0.5),
                };
                self.now[at].x += dx * share_a;
                self.now[at].y += dy * share_a;
                self.now[at + 1].x -= dx * share_b;
                self.now[at + 1].y -= dy * share_b;
            }
            self.now[0] = from;
            self.now[POINTS - 1] = to;
        }
        self.moved = self
            .now
            .iter()
            .zip(&self.before)
            .map(|(now, before)| now.distance(*before))
            .fold(0., f32::max);
    }

    /// Whether it has stopped moving, so nothing needs drawing again until a jack moves.
    pub fn is_still(&self) -> bool {
        self.moved < STILL
    }

    /// Its points, from the first jack to the second.
    pub fn points(&self) -> &[Point] {
        &self.now
    }
}

fn piece(from: Point, to: Point) -> f32 {
    (from.distance(to) * SLACK).max(SHORTEST) / (POINTS - 1) as f32
}

#[cfg(test)]
mod tests {
    use super::{Point, Strand};

    fn lowest(strand: &Strand) -> f32 {
        strand
            .points()
            .iter()
            .map(|point| point.y)
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn a_cable_between_two_still_jacks_comes_to_rest() {
        let mut strand = Strand::new(Point::new(0., 0.), Point::new(300., 0.));
        let mut steps = 0;
        while !strand.is_still() {
            strand.step();
            steps += 1;
            assert!(steps < 4000, "still moving after {steps} steps");
        }
    }

    #[test]
    fn a_cable_at_rest_sags_below_its_jacks_and_keeps_its_ends_on_them() {
        let (from, to) = (Point::new(0., 0.), Point::new(300., 0.));
        let strand = Strand::at_rest(from, to);
        assert!(strand.is_still());
        assert!(lowest(&strand) > 30., "sags by {}", lowest(&strand));
        assert_eq!(strand.points().first(), Some(&from));
        assert_eq!(strand.points().last(), Some(&to));
    }

    #[test]
    fn a_cable_hangs_lowest_in_its_middle() {
        let strand = Strand::at_rest(Point::new(0., 0.), Point::new(300., 0.));
        let points = strand.points();
        let middle = points[points.len() / 2].y;
        assert!(middle > points[2].y && middle > points[points.len() - 3].y);
    }

    #[test]
    fn moving_a_jack_sets_a_resting_cable_swinging_again() {
        let mut strand = Strand::at_rest(Point::new(0., 0.), Point::new(300., 0.));
        strand.pin(Point::new(0., 0.), Point::new(200., 120.));
        assert!(!strand.is_still());
        assert_eq!(strand.points().last(), Some(&Point::new(200., 120.)));
        strand.step();
        assert_eq!(strand.points().last(), Some(&Point::new(200., 120.)));
    }

    #[test]
    fn pinning_to_where_it_already_is_leaves_a_resting_cable_still() {
        let mut strand = Strand::at_rest(Point::new(0., 0.), Point::new(300., 0.));
        strand.pin(Point::new(0., 0.), Point::new(300., 0.));
        assert!(strand.is_still());
    }

    #[test]
    fn jacks_one_above_the_other_still_leave_a_loop() {
        let strand = Strand::at_rest(Point::new(0., 0.), Point::new(0., 10.));
        assert!(lowest(&strand) > 10.);
    }
}
