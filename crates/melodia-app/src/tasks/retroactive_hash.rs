//! Background task that hashes every track row whose `file_hash` column is still NULL. Safe to
//! invoke on every startup (no-op when the column is already populated) and after a folder scan,
//! which leaves unhashed every new file no move could explain (`metadata::Hashing`).
//!
//! On a first import that is the whole library, read end to end — terabytes on a lossless one,
//! hours on a spinning disk. So the pass commits a page at a time and stops at shutdown, the next
//! launch resuming at whatever is still NULL, and only one runs at once: boot and every completed
//! scan both spawn it, and two passes over the same rows would read every file twice.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio_util::sync::CancellationToken;

use crate::state::AppState;
use crate::tasks::TaskSpawner;
use melodia_core::error::AppResult;
use melodia_store::database::DbPool;
use melodia_store::database::queries;
use melodia_store::media::ingest::scan_pool::ScanPool;

/// Rows hashed and committed together: the most a quit or a crash costs, and the paths resident at
/// once.
const PAGE_ROWS: i64 = 256;

/// Whether a pass is running.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Whether a spawn has asked since the running pass last read its work list.
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Hash what is unhashed, or have the pass already running look again once it is done.
pub fn spawn(spawner: &TaskSpawner, state: &AppState) {
    REQUESTED.store(true, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let db = state.db.clone();
    spawner.spawn_cancellable(|shutdown| async move {
        loop {
            while !shutdown.is_cancelled() && REQUESTED.swap(false, Ordering::SeqCst) {
                if let Err(e) = hash_unhashed_tracks(&db, &shutdown, PAGE_ROWS).await {
                    log::warn!("Background hashing failed: {}", melodia_core::error::describe(&e));
                }
            }
            RUNNING.store(false, Ordering::SeqCst);
            // A spawn landing between the last `swap` and the `store` saw a pass running and left.
            let missed = REQUESTED.load(Ordering::SeqCst);
            if shutdown.is_cancelled() || !missed || RUNNING.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

/// Hash every track missing a `file_hash`, a page at a time, answering with how many were written.
///
/// Keyset by id, so a row whose file is gone, and stays NULL, is stepped over rather than handed
/// back on every page. `page_rows` is a parameter so a test can reach a second page at all.
async fn hash_unhashed_tracks(
    db: &DbPool,
    shutdown: &CancellationToken,
    page_rows: i64,
) -> AppResult<usize> {
    let mut after_id = 0;
    let mut hashed = 0;

    while !shutdown.is_cancelled() {
        let page = queries::track::get_unhashed_track_paths_after(db, after_id, page_rows).await?;
        let Some(last_id) = page.last().map(|(id, _)| *id) else {
            break;
        };
        if after_id == 0 {
            log::info!("Retroactive hashing started");
        }
        after_id = last_id;

        let shutdown = shutdown.clone();
        let updates = tokio::task::spawn_blocking(move || {
            ScanPool::for_files(page.len()).install(|| hash_each(&page, &shutdown))
        })
        .await
        .map_err(|e| melodia_core::error::AppError::scanner("Hashing task panicked", e))?;

        queries::track::batch_update_hashes(db, &updates).await?;
        hashed += updates.len();
    }

    if hashed > 0 {
        log::info!("Retroactive hashing wrote {hashed} hash(es)");
    }
    Ok(hashed)
}

/// **Blocking.** A file not yet started when shutdown fires is left for the next launch, so the
/// page in flight costs at most one file per thread.
fn hash_each(
    unhashed: &[(i64, String)],
    shutdown: &CancellationToken,
) -> Vec<(i64, String, Option<String>)> {
    use rayon::prelude::*;

    unhashed
        .par_iter()
        .filter_map(|(id, path_str)| {
            if shutdown.is_cancelled() {
                return None;
            }
            let path = Path::new(path_str);
            // One `stat` answers both "is it still there" and "when was it last
            // written" — an absent file fails here exactly as the old
            // `path.exists()` check did, and the mtime comes from the same
            // instant as that existence proof.
            let Ok(meta) = std::fs::metadata(path) else {
                log::debug!("Skipping missing file during retroactive hash: {path_str}");
                return None;
            };

            let hash = match melodia_store::media::ingest::metadata::compute_file_hash(path) {
                Ok(h) => h,
                Err(e) => {
                    log::warn!("Failed to hash {path_str}: {}", melodia_core::error::describe(&e));
                    return None;
                }
            };

            let mtime = melodia_store::media::ingest::metadata::date_modified_from_metadata(&meta);

            Some((*id, hash, mtime))
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/retroactive_hash_tests.rs"]
mod tests;
