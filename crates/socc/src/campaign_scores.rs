use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    Database, RedisConnection, event,
    mappack::{self, AnyMappackId},
    opt_event::OptEvent,
    redis_key::{mappack_key, mappacks_key},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, StreamTrait, TransactionTrait};

#[tracing::instrument(skip(conn, redis_conn), fields(mappack = %mappack.mappack_id()))]
async fn update_mappack<C: TransactionTrait + Sync>(
    conn: &C,
    redis_conn: &mut RedisConnection,
    mappack: AnyMappackId<'_>,
    event: OptEvent<'_>,
) -> anyhow::Result<()> {
    let rows = mappack::update_mappack(conn, redis_conn, mappack, event).await?;
    tracing::info!("Rows: {rows}");
    Ok(())
}

async fn update_event_mappacks<C>(conn: &C, redis_conn: &mut RedisConnection) -> anyhow::Result<()>
where
    C: ConnectionTrait + StreamTrait + TransactionTrait + Sync,
{
    for event in event::event_list(conn, true).await? {
        for edition in event::event_editions_list(conn, &event.handle).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );

            let mappack = AnyMappackId::Event(&event.event, &edition);

            let _: () = redis_conn.del(mappack_key(mappack)).await?;

            for map in event::event_edition_maps(conn, event.event.id, edition.id).await? {
                let _: () = redis_conn.sadd(mappack_key(mappack), map.game_id).await?;
            }

            update_mappack(
                conn,
                redis_conn,
                mappack,
                OptEvent::new(&event.event, &edition),
            )
            .await?;
        }
    }

    Ok(())
}

pub async fn update(db: Database) -> anyhow::Result<()> {
    let mut redis_conn = db.redis_pool.get().await?;
    let conn = DatabaseConnection::from(db.mysql_pool);

    update_event_mappacks(&conn, &mut redis_conn).await?;

    let mappacks: Vec<String> = redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(
            &conn,
            &mut redis_conn,
            AnyMappackId::Id(&mappack_id),
            Default::default(),
        )
        .await?;
    }

    Ok(())
}
