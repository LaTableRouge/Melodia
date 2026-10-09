//! One-shot merge for albums that were split by per-track performers before ingest learned to
//! group by folder.

use crate::state::AppState;
use crate::tasks::{TaskSpawner, one_shot};
use melodia_core::error::AppResult;
use melodia_store::database::queries;

/// Run the merge unless this install has already had one.
pub fn spawn(spawner: &TaskSpawner, state: &AppState) {
    one_shot::spawn(
        spawner,
        state,
        one_shot::Sweep {
            label: "Album folder consolidation",
            marker: "albums_consolidated_by_folder",
            done: |flags| flags.albums_consolidated_by_folder,
            mark: |flags| flags.albums_consolidated_by_folder = true,
            on_failure: one_shot::OnFailure::Mark,
        },
        |state| async move { consolidate(&state).await },
    );
}

async fn consolidate(state: &AppState) -> AppResult<()> {
    let mut tx = state.db.write().begin().await?;
    let retired = queries::album::consolidate_split_albums_in_folders(&mut tx).await?;
    tx.commit().await?;
    if retired > 0 {
        log::info!("Album consolidation: merged {retired} duplicate album row(s)");
    }
    Ok(())
}
