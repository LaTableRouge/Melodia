use crate::database::DbPool;
use crate::database::queries;
#[allow(clippy::wildcard_imports)]
use crate::database::queries::fixtures::*;
use melodia_core::entities::scan::{ExtractedMetadata, ReleaseTags};
use melodia_core::entities::tags::ClearedReleaseTags;
use melodia_core::error::AppError;

#[tokio::test]
async fn get_all_albums_from_seeded_db() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let albums = queries::album::get_all_albums(&db).await?;
    assert_eq!(albums.len(), 2); // "Album One" and "Album Two"
    // Sorted by name — "Album One" before "Album Two"
    assert_eq!(albums[0].name, "Album One");
    assert_eq!(albums[1].name, "Album Two");
    Ok(())
}

#[tokio::test]
async fn get_album_by_id_happy_path() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let albums = queries::album::get_all_albums(&db).await?;
    let found = queries::album::get_album_by_id(&db, albums[0].id).await?;
    assert_eq!(found.name, albums[0].name);
    Ok(())
}

#[tokio::test]
async fn get_album_by_id_not_found() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let result = queries::album::get_album_by_id(&db, 99999).await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
async fn get_albums_by_artist() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let artist_id: i64 = sqlx::query_scalar("SELECT id FROM artists WHERE name = 'Artist A'")
        .fetch_one(db.read())
        .await?;
    let albums = queries::album::get_albums_by_artist(&db, artist_id).await?;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "Album One");
    Ok(())
}

async fn album_id_named(db: &DbPool, name: &str) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar("SELECT id FROM albums WHERE name = ?")
        .bind(name)
        .fetch_one(db.read())
        .await?)
}

async fn a_track_on(db: &DbPool, album_id: i64) -> Result<i64, AppError> {
    Ok(sqlx::query_scalar("SELECT id FROM tracks WHERE album_id = ? LIMIT 1")
        .bind(album_id)
        .fetch_one(db.read())
        .await?)
}

/// With two albums, the one not playing is the only answer, so a pick that could land back on
/// the playing album would fail this within a few presses.
#[tokio::test]
async fn random_album_id_never_picks_the_playing_album() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let one = album_id_named(&db, "Album One").await?;
    let two = album_id_named(&db, "Album Two").await?;
    let playing = a_track_on(&db, one).await?;

    for _ in 0..16 {
        assert_eq!(queries::album::random_album_id(&db, Some(playing)).await?, Some(two));
    }
    Ok(())
}

/// An album row outlives its last track until the orphan purge runs, and picking one plays nothing.
#[tokio::test]
async fn random_album_id_skips_an_album_with_no_tracks() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let one = album_id_named(&db, "Album One").await?;
    let two = album_id_named(&db, "Album Two").await?;
    sqlx::query("DELETE FROM tracks WHERE album_id = ?").bind(two).execute(db.write()).await?;
    let playing = a_track_on(&db, one).await?;

    assert_eq!(queries::album::random_album_id(&db, Some(playing)).await?, None);
    assert_eq!(queries::album::random_album_id(&db, None).await?, Some(one), "nothing playing");
    Ok(())
}

/// An album's date is the earliest file mtime among its tracks, not when the scanner indexed them.
#[tokio::test]
async fn get_album_dates_added_uses_the_earliest_file_mtime() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let one = album_id_named(&db, "Album One").await?;
    let two = album_id_named(&db, "Album Two").await?;
    sqlx::query(
        "UPDATE tracks SET date_modified = '2020-01-01T00:00:00+00:00', \
         date_added = '2025-06-01T00:00:00+00:00'",
    )
    .execute(db.write())
    .await?;
    let track = a_track_on(&db, one).await?;
    sqlx::query(
        "UPDATE tracks SET date_modified = '2018-03-15T12:00:00+00:00', \
         date_added = '2025-06-01T00:00:00+00:00' WHERE id = ?",
    )
    .bind(track)
    .execute(db.write())
    .await?;

    let dates = queries::album::get_album_dates_added(&db).await?;

    assert_eq!(dates.get(&one).map(String::as_str), Some("2018-03-15T12:00:00+00:00"));
    assert_eq!(dates.get(&two).map(String::as_str), Some("2020-01-01T00:00:00+00:00"));
    Ok(())
}

#[tokio::test]
async fn get_all_albums_track_counts() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let albums = queries::album::get_all_albums(&db).await?;
    // "Album One" has 2 tracks (track1 + track3), "Album Two" has 1
    let one = albums
        .iter()
        .find(|a| a.name == "Album One")
        .ok_or_else(|| AppError::Validation("Album One missing".into()))?;
    let two = albums
        .iter()
        .find(|a| a.name == "Album Two")
        .ok_or_else(|| AppError::Validation("Album Two missing".into()))?;
    assert_eq!(one.track_count, 2);
    assert_eq!(two.track_count, 1);
    Ok(())
}

#[tokio::test]
async fn set_album_artwork_replaces_existing_cover() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    let album_id: i64 = sqlx::query_scalar("SELECT id FROM albums WHERE name = 'Album One'")
        .fetch_one(db.read())
        .await?;

    // Give the album an existing cover — the roll-up refuses to touch this case.
    sqlx::query("UPDATE albums SET artwork_path = ? WHERE id = ?")
        .bind("/covers/old.jpg")
        .bind(album_id)
        .execute(db.write())
        .await?;

    let mut tx = db.write().begin().await?;
    queries::album::set_album_artwork(&mut tx, &[album_id], Some("/covers/new.jpg")).await?;
    tx.commit().await?;

    let art: Option<String> = sqlx::query_scalar("SELECT artwork_path FROM albums WHERE id = ?")
        .bind(album_id)
        .fetch_one(db.read())
        .await?;
    assert_eq!(art.as_deref(), Some("/covers/new.jpg"));
    Ok(())
}

#[tokio::test]
async fn prune_orphans_removes_emptied_album_artist_and_genre() -> Result<(), AppError> {
    let db = setup_seeded_db().await?;
    // "Album Two" / "Artist B" / genre "Pop" each have a single track (track2);
    // deleting it strands all three, while genre "Rock" (track1/track3) survives.
    let mut tx = db.write().begin().await?;
    let deleted = queries::scan::delete_track_by_path(&mut tx, "/music/track2.mp3").await?;
    assert!(deleted);
    queries::scan::prune_orphans(&mut tx).await?;
    tx.commit().await?;

    let albums: Vec<String> =
        sqlx::query_scalar("SELECT name FROM albums ORDER BY name").fetch_all(db.read()).await?;
    assert_eq!(albums, vec!["Album One".to_owned()]);

    let artist_b: Option<i64> =
        sqlx::query_scalar("SELECT id FROM artists WHERE name = 'Artist B'")
            .fetch_optional(db.read())
            .await?;
    assert!(artist_b.is_none(), "orphaned Artist B should be pruned");

    // A still-used artist and the id-1 "unknown" default both survive.
    let artist_a: Option<i64> =
        sqlx::query_scalar("SELECT id FROM artists WHERE name = 'Artist A'")
            .fetch_optional(db.read())
            .await?;
    assert!(artist_a.is_some());
    let unknown: Option<i64> =
        sqlx::query_scalar("SELECT id FROM artists WHERE id = 1").fetch_optional(db.read()).await?;
    assert!(unknown.is_some(), "the id-1 unknown default must never be pruned");

    // The emptied genre is pruned; a still-used genre survives.
    let genres: Vec<String> =
        sqlx::query_scalar("SELECT name FROM genres ORDER BY name").fetch_all(db.read()).await?;
    assert_eq!(genres, vec!["Rock".to_owned()]);

    Ok(())
}

// === Release-level columns ===

/// A track carrying every release-level tag, so the album it seeds has something to clear.
fn released(title: &str) -> ExtractedMetadata {
    let mut meta = make_test_metadata(title);
    meta.release = ReleaseTags {
        label: Some("ECM".to_owned()),
        catalog_number: Some("ECM 1064".to_owned()),
        barcode: Some("042281100420".to_owned()),
        media: Some("CD".to_owned()),
        release_type: Some("Album".to_owned()),
        release_country: Some("DE".to_owned()),
        musicbrainz_release_group_id: None,
        is_compilation: true,
    };
    meta
}

/// The six text columns and the flag, as they sit on the row.
async fn stored_release(db: &DbPool, album_id: i64) -> Result<Release, AppError> {
    let row = sqlx::query_as::<_, Release>(
        "SELECT label, catalog_number, barcode, media, release_type, release_country, \
                is_compilation FROM albums WHERE id = ?",
    )
    .bind(album_id)
    .fetch_one(db.read())
    .await?;
    Ok(row)
}

type Release = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    bool,
);

async fn seed_released_album() -> Result<(DbPool, i64), AppError> {
    let db = DbPool::test_pool().await?;
    queries::folder::insert_folder(&db, "/music", true).await?;
    insert_tagged_track(&db, "/music/1.mp3", &released("One")).await?;

    let albums = queries::album::get_all_albums(&db).await?;
    let id = albums.first().map(|a| a.id).ok_or_else(|| missing("the seeded album"))?;
    Ok((db, id))
}

fn missing(what: &str) -> AppError {
    AppError::Validation(format!("missing {what}"))
}

/// **`upsert_album` structurally cannot empty one of these.** It coalesces a NULL away so a track
/// saying nothing doesn't blank what its neighbours said, so without this pass a release goes on
/// showing a label no file carries. The flag is the worse half: its upsert is an `OR`, which no
/// re-ingest can ever bring back down.
#[tokio::test]
async fn every_release_column_can_be_emptied() -> Result<(), AppError> {
    let (db, album_id) = seed_released_album().await?;
    assert_eq!(
        stored_release(&db, album_id).await?,
        (
            Some("ECM".to_owned()),
            Some("ECM 1064".to_owned()),
            Some("042281100420".to_owned()),
            Some("CD".to_owned()),
            Some("Album".to_owned()),
            Some("DE".to_owned()),
            true,
        )
    );

    let mut tx = db.write().begin().await?;
    queries::album::clear_release_tags(
        &mut tx,
        &[album_id],
        ClearedReleaseTags {
            label: true,
            catalog_number: true,
            barcode: true,
            media: true,
            release_type: true,
            release_country: true,
            compilation: true,
        },
    )
    .await?;
    tx.commit().await?;

    assert_eq!(stored_release(&db, album_id).await?, (None, None, None, None, None, None, false));
    Ok(())
}

/// A flag left `false` leaves its column reading itself, which is what lets one statement carry
/// seven independent decisions.
#[tokio::test]
async fn a_column_the_edit_did_not_clear_keeps_its_value() -> Result<(), AppError> {
    let (db, album_id) = seed_released_album().await?;

    let mut tx = db.write().begin().await?;
    queries::album::clear_release_tags(
        &mut tx,
        &[album_id],
        ClearedReleaseTags { label: true, ..ClearedReleaseTags::default() },
    )
    .await?;
    tx.commit().await?;

    let stored = stored_release(&db, album_id).await?;
    assert_eq!(stored.0, None, "the one column the edit named");
    assert_eq!(stored.1, Some("ECM 1064".to_owned()));
    assert!(stored.6, "the flag is not swept along with a text column");
    Ok(())
}

/// Both early returns, since each would otherwise run an `UPDATE` whose `SET` list nulls nothing
/// and whose `IN ()` matches nothing — free, but only by accident.
#[tokio::test]
async fn an_edit_that_cleared_nothing_touches_no_row() -> Result<(), AppError> {
    let (db, album_id) = seed_released_album().await?;
    let before = stored_release(&db, album_id).await?;

    let mut tx = db.write().begin().await?;
    queries::album::clear_release_tags(&mut tx, &[album_id], ClearedReleaseTags::default()).await?;
    queries::album::clear_release_tags(
        &mut tx,
        &[],
        ClearedReleaseTags { label: true, ..ClearedReleaseTags::default() },
    )
    .await?;
    tx.commit().await?;

    assert_eq!(stored_release(&db, album_id).await?, before);
    Ok(())
}
