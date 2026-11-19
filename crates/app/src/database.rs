//! The app's database (`patchbay_core::database`), opened once at launch: `data.sqlite` beside the settings,
//! or in memory for captures, so a frame never touches anyone's data.

use gpui_kit::*;
use patchbay_core::database::Database;

use crate::settings;

struct Data(
    #[expect(dead_code, reason = "opened now; the tool catalog cache reads it next")]
    Option<Database>,
);

impl Global for Data {}

pub fn init(cx: &mut App) {
    let opened = match settings::file("data.sqlite") {
        Some(path) => Database::open(&path),
        None => Database::in_memory(),
    };
    let database = opened
        .inspect_err(|error| tracing::error!("no database: {error}"))
        .ok();
    cx.set_global(Data(database));
}
