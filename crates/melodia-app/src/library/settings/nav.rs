//! Sidebar section visibility toggles.

use crate::services;
use crate::state::AppState;
use melodia_core::error::AppError;

pub fn set_hide_browse(state: &AppState, hide: bool) -> Result<(), AppError> {
    services::settings::mutate_settings(&state.paths, move |s| {
        s.nav.hide_browse = hide;
    })
}

pub fn set_hide_favorites(state: &AppState, hide: bool) -> Result<(), AppError> {
    services::settings::mutate_settings(&state.paths, move |s| {
        s.nav.hide_favorites = hide;
    })
}

pub fn set_hide_recently_played(state: &AppState, hide: bool) -> Result<(), AppError> {
    services::settings::mutate_settings(&state.paths, move |s| {
        s.nav.hide_recently_played = hide;
    })
}

pub fn set_hide_my_library(state: &AppState, hide: bool) -> Result<(), AppError> {
    services::settings::mutate_settings(&state.paths, move |s| {
        s.nav.hide_my_library = hide;
    })
}
