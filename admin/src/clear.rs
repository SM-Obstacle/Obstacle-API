use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    acquire,
    context::{Context, Ctx as _},
    mappack::AnyMappackId,
    models, must,
    redis_key::mappack_key,
    Database, DatabaseConnection,
};

#[derive(clap::Args)]
pub struct ClearCommand {
    event_handle: String,
    event_edition: u32,
}

#[tracing::instrument(skip(db))]
pub async fn clear_content(
    db: &mut DatabaseConnection<'_>,
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
        .execute(&mut **db.mysql_conn)
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
    let mut conn = acquire!(db?);

    let (event, edition) = must::have_event_edition(
        conn.mysql_conn,
        Context::default()
            .with_event_handle(&event_handle)
            .with_edition_id(event_edition),
    )
    .await?;

    clear_content(&mut conn, &event, &edition).await?;

    Ok(())
}
