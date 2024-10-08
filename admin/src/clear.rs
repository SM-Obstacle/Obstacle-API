use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    mappack::AnyMappackId, models, must, redis_key::mappack_key, Database, DatabaseConnection,
};

#[derive(clap::Args)]
pub struct ClearCommand {
    event_handle: String,
    event_edition: u32,
}

#[tracing::instrument(skip(db))]
pub async fn clear_content(
    db: &mut DatabaseConnection,
    event: &models::Event,
    edition: &models::EventEdition,
) -> anyhow::Result<()> {
    let _: () = db
        .redis_conn
        .del(mappack_key(AnyMappackId::Event(event, edition)))
        .await?;

    sqlx::query("delete from event_edition_maps where event_id = ? and edition_id = ?")
        .bind(event.id)
        .bind(edition.id)
        .execute(&mut *db.mysql_conn)
        .await?;

    Ok(())
}

pub async fn clear(
    db: Database,
    ClearCommand {
        event_handle,
        event_edition,
    }: ClearCommand,
) -> anyhow::Result<()> {
    let mut conn = db.acquire().await?;

    let (event, edition) =
        must::have_event_edition(&mut conn.mysql_conn, &event_handle, event_edition).await?;

    clear_content(&mut conn, &event, &edition).await?;

    Ok(())
}
