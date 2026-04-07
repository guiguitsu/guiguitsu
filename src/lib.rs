pub mod config;
pub mod git_utils;
pub mod jujutsu;
pub mod stacks;
pub mod cli;

#[cfg(feature = "gui")]
slint::include_modules!();

#[cfg(feature = "gui")]
pub mod models;

#[cfg(feature = "gui")]
pub mod gui;

pub fn verbose() -> bool {
    std::env::var("VERBOSE").as_deref() == Ok("1")
}
