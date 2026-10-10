//! Album library API — thin wrappers over the `queries::album` /
//! `queries::track` layer, mirroring `library/tracks.rs`. Each function
//! takes `&AppState` and returns `Result<_, AppError>`; the UI bridge
//! (`melodia-views`' `ui/albums/`) does the in-memory sorting / filtering / chunking.

use std::collections::HashMap;

use crate::state::AppState;
use melodia_core::entities::{album, track};
use melodia_core::error::AppError;
use melodia_store::database::queries;

/// Every album, name-sorted (the `album_stats` view's default order). The
/// Albums grid re-sorts in memory for the Year / Artist sort options, so
/// this is fetched once and cached UI-side.
pub async fn get_albums(state: &AppState) -> Result<Vec<album::AlbumStats>, AppError> {
    queries::album::get_all_albums(&state.db).await
}

/// Each album's earliest `date_added` among its tracks, for the grid's date-added sort. Kept out
/// of `AlbumStats` so the view every other album query reads stays as it is.
pub async fn get_album_dates_added(state: &AppState) -> Result<HashMap<i64, String>, AppError> {
    queries::album::get_album_dates_added(&state.db).await
}

/// Stats for a single album, or `AppError::NotFound` if the id is gone
/// (e.g. the album's folder was removed between grid render and click).
pub async fn get_album_detail(state: &AppState, id: i64) -> Result<album::AlbumStats, AppError> {
    queries::album::get_album_by_id(&state.db, id).await
}

/// Every track on an album, disc/track-number ordered, in the list-view
/// projection the shared `TrackList` component renders.
pub async fn get_album_tracks(
    state: &AppState,
    album_id: i64,
) -> Result<Vec<track::TrackListRow>, AppError> {
    queries::track::get_tracks_by_album_for_list(&state.db, album_id).await
}
