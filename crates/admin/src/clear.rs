use deadpool_redis::redis::AsyncCommands;
use entity::{event, event_edition, event_edition_maps};
use records_lib::{Database, RedisConnection, mappack::AnyMappackId, must, redis_key::mappack_key};
use sea_orm::{
    ColumnTrait as _, ConnectionTrait, DatabaseConnection, EntityTrait as _, QueryFilter as _,
};

#[derive(clap::Args)]
pub struct ClearCommand {
    event_handle: String,
    event_edition: u32,
}

#[tracing::instrument(skip(conn, redis_conn))]
pub async fn clear_content<C: ConnectionTrait>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    event: &event::Model,
    edition: &event_edition::Model,
) -> anyhow::Result<()> {
    let _: () = redis_conn
        .del(mappack_key(AnyMappackId::Event(event, edition)))
        .await?;

    event_edition_maps::Entity::delete_many()
        .filter(
            event_edition_maps::Column::EventId
                .eq(event.id)
                .and(event_edition_maps::Column::EditionId.eq(edition.id)),
        )
        .exec(conn)
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
    let conn = DatabaseConnection::from(db.mysql_pool);
    let mut redis_conn = db.redis_pool.get().await?;

    let (event, edition) = must::have_event_edition(&conn, &event_handle, event_edition).await?;

    clear_content(&conn, &mut redis_conn, &event, &edition).await?;

    Ok(())
}
