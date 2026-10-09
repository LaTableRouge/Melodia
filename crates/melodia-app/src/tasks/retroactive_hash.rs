//! Background task that hashes every track row whose `file_hash` column is still NULL. Safe to
//! invoke on every startup (no-op when the column is already populated) and after a folder scan,
//! which leaves unhashed every new file no move could explain (`metadata::Hashing`).
//!
//! On a first import that is the whole library, read end to end — terabytes on a lossless one,
//! hours on a spinning disk. So the pass commits a page at a time and stops at shutdown, the next
//! launch resuming at whatever is still NULL, and only one runs at once: boot and every completed
//! scan both spawn it, and two passes over the same rows would read every file twice.
//!
//! It runs for hours while the user listens, so it reads one file at a time and paces itself while
//! anything plays. A parallel pass at full speed takes every core and the disk's whole queue, and
//! the decoder, reading the playing file off the same disk, starves: playback stutters.

use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::state::AppState;
use crate::tasks::TaskSpawner;
use melodia_core::error::AppResult;
use melodia_engine::player::engine::types::PlaybackStatus;
use melodia_store::database::DbPool;
use melodia_store::database::queries;
use melodia_store::media::ingest::metadata;

/// Rows hashed and committed together: the most a quit or a crash costs, and the paths resident at
/// once.
const PAGE_ROWS: i64 = 256;

/// One read between pauses.
const CHUNK_BYTES: usize = 1 << 20;

/// The pause after each chunk while music plays, holding the pass to about 10 MiB/s: a small
/// fraction of any disk, and still a 30 MB FLAC every few seconds.
const PLAYING_PAUSE: Duration = Duration::from_millis(100);

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
    let player_state = state.player_state.clone();
    let playing =
        move || player_state.status_atomic.load(Ordering::Relaxed) == PlaybackStatus::Playing as u8;
    spawner.spawn_cancellable(|shutdown| async move {
        loop {
            while !shutdown.is_cancelled() && REQUESTED.swap(false, Ordering::SeqCst) {
                if let Err(e) = hash_unhashed_tracks(&db, &shutdown, PAGE_ROWS, &playing).await {
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
async fn hash_unhashed_tracks<P>(
    db: &DbPool,
    shutdown: &CancellationToken,
    page_rows: i64,
    playing: &P,
) -> AppResult<usize>
where
    P: Fn() -> bool + Clone + Send + Sync + 'static,
{
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
        let playing = playing.clone();
        let updates = tokio::task::spawn_blocking(move || hash_each(&page, &shutdown, &playing))
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

/// **Blocking.** Shutdown abandons the file in flight, leaving it for the next launch.
fn hash_each(
    unhashed: &[(i64, String)],
    shutdown: &CancellationToken,
    playing: &impl Fn() -> bool,
) -> Vec<(i64, String, Option<String>)> {
    let between_chunks = || {
        if shutdown.is_cancelled() {
            return ControlFlow::Break(());
        }
        if playing() {
            std::thread::sleep(PLAYING_PAUSE);
        }
        ControlFlow::Continue(())
    };

    let mut updates = Vec::with_capacity(unhashed.len());
    for (id, path_str) in unhashed {
        if shutdown.is_cancelled() {
            break;
        }
        let path = Path::new(path_str);
        // One `stat` answers both "is it still there" and "when was it last written".
        let Ok(meta) = std::fs::metadata(path) else {
            log::debug!("Skipping missing file during retroactive hash: {path_str}");
            continue;
        };

        let hash = match metadata::compute_file_hash_paced(path, CHUNK_BYTES, between_chunks) {
            Ok(Some(hash)) => hash,
            Ok(None) => break,
            Err(e) => {
                log::warn!("Failed to hash {path_str}: {}", melodia_core::error::describe(&e));
                continue;
            }
        };

        updates.push((*id, hash, metadata::date_modified_from_metadata(&meta)));
    }
    updates
}

#[cfg(test)]
#[path = "tests/retroactive_hash_tests.rs"]
mod tests;
