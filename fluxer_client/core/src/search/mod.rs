//! Pure search and autocomplete logic for the app: quick switcher ranking
//! (`switcher`), emoji search/frecency and composer trigger parsing (`emoji`).
//! No UI types; everything reads `ClientState` and returns plain data.

pub mod emoji;
pub mod switcher;

#[cfg(test)]
pub(crate) mod fixture;
