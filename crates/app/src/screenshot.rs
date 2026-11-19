use std::time::Duration;

use gpui_kit::*;

/// Draws every window, since hidden ones don't draw on their own.
fn redraw(cx: &mut AsyncApp) {
    cx.update(|cx| {
        for handle in cx.windows() {
            let _ = handle.update(cx, |_, window, cx| {
                window.refresh();
                let _ = window.draw(cx);
            });
        }
    });
}

/// Clicks at window coordinates, holding `modifiers`, by way of the same input path as a real mouse.
fn click(window: &mut Window, cx: &mut App, x: f32, y: f32, modifiers: Modifiers) {
    let position = point(px(x), px(y));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    let _ = window.draw(cx);
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// Performs `PATCHBAY_KEYS` in the first window, saves a settled frame of the newest one (Settings, after
/// `cmd-,`) to `PATCHBAY_SHOT` and quits, so the UI can be driven
/// and checked without a screen or screen recording permission. Steps are separated by spaces: a key such as `j` or
/// `enter`, `click:X,Y` in window points (half the frame's pixels, which are 2x), with modifiers like `cmd-click:X,Y`,
/// or `wait` for half a second.
pub fn capture(cx: &mut App) {
    let path = std::env::var("PATCHBAY_SHOT").expect("PATCHBAY_SHOT is not set");
    cx.spawn(async move |cx| {
        let frame = Duration::from_millis(100);
        for _ in 0..10 {
            redraw(cx);
            cx.background_executor().timer(frame).await;
        }
        // The keystroke parser wants a static string, and this process exits shortly.
        let keys = std::env::var("PATCHBAY_KEYS").unwrap_or_default().leak();
        for step in keys.split_whitespace() {
            if step == "wait" {
                for _ in 0..5 {
                    redraw(cx);
                    cx.background_executor().timer(frame).await;
                }
                continue;
            }
            cx.update(|cx| {
                cx.windows()[0]
                    .update(cx, |_, window, cx| {
                        match step.split_once("click:") {
                            Some((held, place)) => {
                                let (x, y) = place.split_once(',').expect("click:X,Y");
                                let (x, y) = (x.parse().expect("X"), y.parse().expect("Y"));
                                // `cmd-` and the like, read as the modifiers of a keystroke, which wants a
                                // static string, like the keys above.
                                let modifiers = Keystroke::parse(format!("{held}a").leak())
                                    .expect("bad modifiers before click:")
                                    .modifiers;
                                click(window, cx, x, y, modifiers);
                            }
                            None => {
                                window.dispatch_keystroke(
                                    Keystroke::parse(step).expect("bad key"),
                                    cx,
                                );
                            }
                        }
                        window.refresh();
                        let _ = window.draw(cx);
                    })
                    .expect("window closed");
            });
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
        }
        // A hidden window isn't drawing, so keep it going for what takes frames to load.
        for _ in 0..10 {
            redraw(cx);
            cx.background_executor().timer(frame).await;
        }
        cx.update(|cx| {
            let window = *cx.windows().last().expect("no window");
            window
                .update(cx, |_, window, cx| {
                    window.refresh();
                    let _ = window.draw(cx);
                    window.render_to_image()
                })
                .expect("window closed")
                .expect("frame capture failed")
                .save(&path)
                .expect("failed to save screenshot");
            cx.quit();
        })
    })
    .detach();
}
