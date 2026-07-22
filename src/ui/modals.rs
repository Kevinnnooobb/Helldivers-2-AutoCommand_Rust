mod context;
mod library_context;
mod capture;
mod settings;
mod stratagem_settings;

pub use context::*;
pub use library_context::*;
pub use capture::*;
pub use settings::*;
pub use stratagem_settings::*;

pub(crate) fn cat_label(cat: &str) -> String {
    let short = crate::ui::common::cat_short(cat);
    if short == "?" { cat.to_string() } else { format!("{short} ({cat})") }
}
