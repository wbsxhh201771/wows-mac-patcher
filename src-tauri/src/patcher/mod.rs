//! The patch engine: PE lookup, install discovery, state machine, backups,
//! AppleDouble cleanup, and diagnostics.
//!
//! This module has **no Tauri dependency** on purpose: `cargo test --lib` can
//! verify patching behaviour without a GUI. The test suite is the regression
//! net for offsets, states, and backup invariants.

pub mod appledouble;
pub mod diag;
pub mod dto;
pub mod fsutil;
pub mod locate;
pub mod manifest;
pub mod patch;
pub mod pe;

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests;
