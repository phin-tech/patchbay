//! The rack's hardware colours, in its two finishes.

use super::*;

/// The hardware's colours, which stay the rack's whatever the theme, in a light and a dark finish: brushed aluminium,
/// or anodised black. The lights take the theme's own colours for good, waiting and failed.
#[derive(Clone, Copy)]
pub(super) struct Look {
    pub(super) cavity: Hsla,
    pub(super) rail: Hsla,
    pub(super) rail_edge: Hsla,
    pub(super) screw: Hsla,
    pub(super) slot: Hsla,
    pub(super) panel_top: Hsla,
    pub(super) panel_bottom: Hsla,
    pub(super) panel_edge: Hsla,
    pub(super) print: Hsla,
    pub(super) print_muted: Hsla,
    pub(super) tape: Hsla,
    pub(super) tape_text: Hsla,
    pub(super) nut: Hsla,
    pub(super) nut_edge: Hsla,
    pub(super) hole: Hsla,
    pub(super) button: Hsla,
    pub(super) shadow: Hsla,
    pub(super) led_off: Hsla,
    pub(super) good: Hsla,
    pub(super) waiting: Hsla,
    pub(super) failed: Hsla,
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Look {
    pub(super) fn of(cx: &App) -> Look {
        let theme = cx.theme();
        let (good, waiting, failed) = (theme.success, theme.warning, theme.danger);
        if theme.mode.is_dark() {
            Look {
                cavity: color(0x0e0f11),
                rail: color(0x1d1e21),
                rail_edge: color(0x34363b),
                screw: color(0x80848a),
                slot: color(0x2a2c30),
                panel_top: color(0x3b3e44),
                panel_bottom: color(0x2a2c31),
                panel_edge: color(0x111214),
                print: color(0xd9dbe0),
                print_muted: color(0x8e939b),
                tape: color(0xe2d7b0),
                tape_text: color(0x2b2616),
                nut: color(0x9a9ea4),
                nut_edge: color(0x3a3d42),
                hole: color(0x050506),
                button: color(0x50545b),
                shadow: rgba(0x0000_0080).into(),
                led_off: color(0x24262a),
                good,
                waiting,
                failed,
            }
        } else {
            Look {
                cavity: color(0x1c1d20),
                rail: color(0x2c2e32),
                rail_edge: color(0x484b51),
                screw: color(0xb0b4ba),
                slot: color(0x4b4f55),
                panel_top: color(0xdfe1e4),
                panel_bottom: color(0xbcc0c5),
                panel_edge: color(0x7d8187),
                print: color(0x26282c),
                print_muted: color(0x5c6067),
                tape: color(0xf3e9c6),
                tape_text: color(0x2b2616),
                nut: color(0xc9ccd1),
                nut_edge: color(0x6c7077),
                hole: color(0x0d0e10),
                button: color(0xeceef0),
                shadow: rgba(0x0000_0059).into(),
                led_off: color(0x6f7378),
                good,
                waiting,
                failed,
            }
        }
    }
}

/// A Cable's colour, one for each client, as a studio colours its leads by where they go.
pub(super) fn lead_color(client: Client) -> Hsla {
    const LEADS: [u32; 10] = [
        0xd2_3c2c, 0xe0_7a1f, 0x2f_9e44, 0xe8_c02a, 0x2f_6fd6, 0x8a_4fd1, 0x1a_a3a3, 0xd6_487e,
        0xe6_e1d6, 0x45_494f,
    ];
    let at = Client::ALL
        .iter()
        .position(|each| *each == client)
        .unwrap_or(0);
    color(LEADS[at % LEADS.len()])
}

/// A hard-wired Cable's colour: plain grey, so it reads as part of the rack rather than a choice.
pub(super) fn wired_color() -> Hsla {
    hsla(0., 0., 0.55, 1.)
}
