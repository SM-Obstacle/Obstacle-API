use deadpool_redis::redis::{self, AsyncCommands};
use records_lib::{
    Database, RedisConnection, event,
    mappack::{self, AnyMappackId},
    opt_event::OptEvent,
    redis_key::{mappack_key, mappacks_key},
};
use sea_orm::{ConnectionTrait, StreamTrait, TransactionTrait};

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

            let mut pipe = redis::pipe();
            let pipe = pipe.atomic();

            pipe.del(mappack_key(mappack));

            for map in event::event_edition_maps(conn, event.event.id, edition.id).await? {
                pipe.sadd(mappack_key(mappack), map.game_id);
            }

            pipe.exec_async(redis_conn).await?;

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

    update_event_mappacks(&db.sql_conn, &mut redis_conn).await?;

    let mappacks: Vec<String> = redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(
            &db.sql_conn,
            &mut redis_conn,
            AnyMappackId::Id(&mappack_id),
            Default::default(),
        )
        .await?;
    }

    tracing::info!("End");

    Ok(())
}
