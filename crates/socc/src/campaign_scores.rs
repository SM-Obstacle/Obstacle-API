use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    Database, DatabaseConnection, acquire, event,
    mappack::{self, AnyMappackId},
    opt_event::OptEvent,
    redis_key::{mappack_key, mappacks_key},
};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

#[tracing::instrument(skip(conn), fields(mappack = %mappack.mappack_id()))]
async fn update_mappack(
    conn: &mut DatabaseConnection<'_>,
    mappack: AnyMappackId<'_>,
    event: OptEvent<'_>,
) -> anyhow::Result<()> {
    let rows = mappack::update_mappack(conn, mappack, event).await?;
    tracing::info!("Rows: {rows}");
    Ok(())
}

async fn update_event_mappacks(conn: &mut DatabaseConnection<'_>) -> anyhow::Result<()> {
    for event in event::event_list(conn.mysql_conn, true).await? {
        for edition in event::event_editions_list(conn.mysql_conn, &event.handle).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );

            let mappack = AnyMappackId::Event(&event.event, &edition);

            let _: () = conn.redis_conn.del(mappack_key(mappack)).await?;

            for map in
                event::event_edition_maps(conn.mysql_conn, event.event.id, edition.id).await?
            {
                let _: () = conn
                    .redis_conn
                    .sadd(mappack_key(mappack), map.game_id)
                    .await?;
            }

            update_mappack(conn, mappack, OptEvent::new(&event.event, &edition)).await?;
        }
    }

    Ok(())
}

pub async fn update(db: Database) -> anyhow::Result<()> {
    let mut conn = acquire!(db?);

    update_event_mappacks(&mut conn).await?;

    let mappacks: Vec<String> = conn.redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(&mut conn, AnyMappackId::Id(&mappack_id), Default::default()).await?;
    }

    Ok(())
}
