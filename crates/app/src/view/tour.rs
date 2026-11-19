//! The tour over the main window, with the steps from `guides`, and running a Command in the front main window from
//! another, as Welcome and Settings do.

use gpui_kit::base::Root;
use typecase::tour::{Tour, TourEvent};

use super::*;

/// The main view in `handle`, if that window is one, beneath the kit's Root.
fn main_view(handle: AnyWindowHandle, cx: &App) -> Option<Entity<MainView>> {
    let root = handle.downcast::<Root>()?;
    let root = root.read(cx).ok()?;
    root.view().clone().downcast::<MainView>().ok()
}

/// Runs a Command in the front main window, or with none open, in a new one: for a window of another kind, such as
/// Welcome, whose buttons ask for what only the main window does.
pub fn run_in_front(name: &'static str, cx: &mut App) {
    let front = cx.active_window().into_iter().chain(cx.windows());
    let mut main = front.filter_map(|handle| Some((handle, main_view(handle, cx)?)));
    let found = main.next().or_else(|| {
        let handle = open(true, cx)?;
        Some((handle, main_view(handle, cx)?))
    });
    let Some((handle, main)) = found else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        window.activate_window();
        main.update(cx, |main, cx| main.run(name, window, cx));
    });
}

impl MainView {
    /// Starts the tour of this window once it's drawn, so what the steps point at is there.
    pub(super) fn begin_tour(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tour.is_some() {
            return;
        }
        self.card = None;
        self.rack.stop_dragging();
        let came_from = self.place;
        let (steps, places): (Vec<_>, Vec<_>) =
            crate::guides::overview(crate::settings::current(cx).patch_bay)
                .into_iter()
                .unzip();
        let tour = cx.new(|cx| Tour::new(steps, cx));
        let heard = cx.subscribe_in(
            &tour,
            window,
            move |this: &mut MainView, _, event: &TourEvent, window, cx| match event {
                TourEvent::Shown(at) => {
                    if let Some(place) = places.get(*at) {
                        this.show(*place, cx);
                    }
                }
                TourEvent::Done | TourEvent::Skipped(_) => this.end_tour(came_from, window, cx),
            },
        );
        self.tour = Some((tour.clone(), heard));
        cx.notify();
        cx.defer_in(window, move |_, window, cx| {
            tour.update(cx, |tour, cx| tour.start(window, cx));
        });
    }

    /// Ends the tour on the place it started from, with the keyboard back in the window.
    fn end_tour(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        self.tour = None;
        self.show(place, cx);
        self.focus.focus(window, cx);
    }
}
