//! The rack's hardware, drawn: plates, ears and screws, tape, legends, lights, switches, buttons and jacks.

use super::*;

/// The plate over the skills, printed with what's below it.
pub(super) fn divider_plate(rect: Rect, look: Look) -> Div {
    place(rect)
        .rounded(px(2.))
        .bg(look.rail)
        .border_1()
        .border_color(look.rail_edge)
        .flex()
        .items_center()
        .justify_center()
        .gap(rems(0.6))
        .child(div().h(px(1.)).w(rems(3.)).bg(look.screw))
        .child(
            div()
                .text_xs()
                .font_bold()
                .text_color(look.screw)
                .child(t!("Skills").to_uppercase()),
        )
        .child(div().h(px(1.)).w(rems(3.)).bg(look.screw))
}

/// A module's front panel: brushed metal, lit from above, with a hairline catching the light along its top.
pub(super) fn panel(rect: Rect, look: Look) -> Div {
    place(rect)
        .rounded(px(3.))
        .border_1()
        .border_color(look.panel_edge)
        .bg(linear_gradient(
            180.,
            linear_color_stop(look.panel_top, 0.),
            linear_color_stop(look.panel_bottom, 1.),
        ))
        .shadow(vec![BoxShadow {
            color: look.shadow,
            offset: point(px(0.), px(2.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(1.))
                .bg(hsla(0., 0., 1., 0.25)),
        )
}

/// A blank plate over a rack space with nothing in it, with a row of vents.
pub(super) fn blank_plate(rect: Rect, rem: f32, look: Look) -> Div {
    let vents = ((rect.width - 2. * rem) / (rem * 0.9)).floor().max(0.) as usize;
    panel(rect, look).opacity(0.85).child(
        h_flex()
            .size_full()
            .justify_center()
            .items_center()
            .gap(px(rem * 0.45))
            .children((0..vents).map(|_| {
                div()
                    .w(px(rem * 0.45))
                    .h(px(rect.height * 0.45))
                    .rounded(px(rem * 0.22))
                    .bg(look.hole)
                    .opacity(0.6)
            })),
    )
}

/// An ear of the rack, the rail its modules are screwed to.
pub(super) fn ear_strip(rect: Rect, look: Look) -> Div {
    place(rect)
        .bg(linear_gradient(
            90.,
            linear_color_stop(look.rail, 0.),
            linear_color_stop(look.rail_edge, 1.),
        ))
        .border_x_1()
        .border_color(look.rail_edge)
}

/// The two screws holding a module to `ear`, level with its top and bottom.
pub(super) fn screws(ear: Rect, module: Rect, rem: f32, look: Look) -> Vec<Div> {
    let across = rem * 0.8;
    let x = ear.x + (ear.width - across) / 2.;
    [
        module.y + rem * 0.6,
        module.y + module.height - rem * 0.6 - across,
    ]
    .into_iter()
    .map(|y| {
        place(Rect {
            x,
            y,
            width: across,
            height: across,
        })
        .rounded_full()
        .bg(look.screw)
        .border_1()
        .border_color(look.slot)
        .flex()
        .items_center()
        .justify_center()
        .child(div().w(px(across * 0.6)).h(px(1.5)).bg(look.slot))
    })
    .collect()
}

/// A strip of label tape with a name written on it, and the letter or number it goes by beside it.
pub(super) fn tape(mark: String, name: String, look: Look, cx: &App) -> Div {
    let mono = cx.theme().mono_font_family.clone();
    h_flex()
        .gap(rems(0.4))
        .items_center()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .min_w(rems(1.3))
                .h(rems(1.3))
                .px(rems(0.2))
                .rounded(px(2.))
                .border_1()
                .border_color(look.print_muted)
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_bold()
                .text_color(look.print)
                .child(mark),
        )
        .child(
            div()
                .relative()
                .min_w_0()
                .px(rems(0.6))
                .py(rems(0.1))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| torn(bounds, look.tape, window),
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .font_family(mono)
                .font_semibold()
                .text_color(look.tape_text)
                .truncate()
                .child(name.to_uppercase()),
        )
}

/// How deep each tooth of a torn end is, as a share of the most, uneven so it reads as torn by hand.
const TEETH: [f32; 7] = [1., 0.45, 0.8, 0.3, 0.95, 0.55, 0.75];

/// A strip of tape filling `bounds`, its two ends torn into uneven teeth, over a shadow a point below it.
fn torn(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let deep = (height * 0.18).min(width / 4.);
    let rows = TEETH.len();
    let shape = |dy: f32| {
        let at = |px_: f32, py: f32| point(px(px_), px(py + dy));
        let mut path = PathBuilder::fill();
        path.move_to(at(x + deep * TEETH[0], y));
        path.line_to(at(x + width - deep * TEETH[rows - 1], y));
        // Down the right end, then back up the left, in and out at each tooth.
        for (row, depth) in TEETH.iter().rev().enumerate() {
            let along = y + height * (row as f32 + 0.5) / rows as f32;
            let edge = if row % 2 == 0 { 0. } else { deep * depth };
            path.line_to(at(x + width - edge, along));
        }
        path.line_to(at(x + width - deep * TEETH[0], y + height));
        path.line_to(at(x + deep * TEETH[rows - 1], y + height));
        for (row, depth) in TEETH.iter().enumerate().rev() {
            let along = y + height * (row as f32 + 0.5) / rows as f32;
            let edge = if row % 2 == 0 { 0. } else { deep * depth };
            path.line_to(at(x + edge, along));
        }
        path.close();
        path.build().ok()
    };
    if let Some(path) = shape(1.) {
        window.paint_path(path, hsla(0., 0., 0., 0.3));
    }
    if let Some(path) = shape(0.) {
        window.paint_path(path, color);
    }
}

/// Small print on the panel, as a legend under a control.
pub(super) fn legend(text: String, look: Look) -> Div {
    div()
        .text_size(rems(9. / 13.))
        .font_semibold()
        .text_color(look.print_muted)
        .whitespace_nowrap()
        .child(text)
}

/// A panel light: lit in `on`'s colour with a glow around it, or dark.
pub(super) fn led(on: Option<Hsla>, look: Look) -> Div {
    let across = rems(0.6);
    let dot = div().size(across).rounded_full().border_1();
    match on {
        Some(color) => dot
            .bg(color)
            .border_color(Hsla {
                l: color.l * 0.6,
                ..color
            })
            .shadow(vec![BoxShadow {
                color: Hsla { a: 0.7, ..color },
                offset: point(px(0.), px(0.)),
                blur_radius: px(6.),
                spread_radius: px(1.),
                inset: false,
            }]),
        None => dot.bg(look.led_off).border_color(look.panel_edge),
    }
}

/// A rocker switch, pressed in at the top when on.
pub(super) fn rocker(on: bool, look: Look) -> Div {
    let half = |pressed: bool| {
        div()
            .h(rems(0.8))
            .w_full()
            .rounded(px(2.))
            .when(pressed, |this| this.bg(look.rail))
            .when(!pressed, |this| {
                this.bg(linear_gradient(
                    180.,
                    linear_color_stop(look.button, 0.),
                    linear_color_stop(look.panel_bottom, 1.),
                ))
            })
            .when(!pressed, |this| {
                this.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.4),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(1.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
    };
    v_flex()
        .w(rems(1.2))
        .h(rems(1.9))
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(3.))
        .bg(look.hole)
        .child(half(on))
        .child(half(!on))
}

/// A small push button with a light in it, lit while it's the one chosen, and its legend beside it.
pub(super) fn push_button(
    id: SharedString,
    label: String,
    chosen: bool,
    look: Look,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .items_center()
        .gap(rems(0.4))
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .w(rems(1.5))
                .h(rems(0.95))
                .rounded(px(2.))
                .border_1()
                .border_color(look.panel_edge)
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(look.button, 0.),
                    linear_color_stop(look.panel_bottom, 1.),
                ))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(rems(0.7))
                        .h(rems(0.22))
                        .rounded(px(1.))
                        .when(chosen, |this| this.bg(look.waiting))
                        .when(!chosen, |this| this.bg(look.led_off)),
                ),
        )
        .child(legend(label, look).when(chosen, |this| this.text_color(look.print)))
}

/// A jack on the panel, centred on `centre` (in the module's own points), with its number or letter printed over it.
pub(super) fn jack(
    id: SharedString,
    centre: Spot,
    mark: String,
    rem: f32,
    look: Look,
) -> Stateful<Div> {
    let across = rem * 1.2;
    let hole = rem * 0.62;
    let hover = look.print;
    v_flex()
        .id(id)
        .absolute()
        .left(px(centre.x - rem))
        .top(px(centre.y - rem * 1.75))
        .w(px(rem * 2.))
        .items_center()
        .cursor(CursorStyle::OpenHand)
        .child(
            div()
                .h(px(rem * 1.15))
                .text_size(rems(9. / 13.))
                .font_bold()
                .text_color(look.print_muted)
                .child(mark),
        )
        .child(
            div()
                .size(px(across))
                .rounded_full()
                .border_1()
                .border_color(look.nut_edge)
                .bg(look.nut)
                .flex()
                .items_center()
                .justify_center()
                .hover(move |this| this.border_color(hover))
                .child(div().size(px(hole)).rounded_full().bg(look.hole)),
        )
}
