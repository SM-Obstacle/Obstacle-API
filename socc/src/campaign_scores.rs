use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    event,
    mappack::{self, AnyMappackId},
    redis_key::{mappack_key, mappacks_key},
    DatabaseConnection,
};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

#[tracing::instrument(skip(conn))]
async fn update_mappack(
    conn: &mut DatabaseConnection,
    mappack: AnyMappackId<'_>,
) -> anyhow::Result<()> {
    let rows = mappack::update_mappack(mappack, conn).await?;
    tracing::info!("Rows: {rows}");
    Ok(())
}

async fn update_event_mappacks(conn: &mut DatabaseConnection) -> anyhow::Result<()> {
    for event in event::event_list(&mut conn.mysql_conn).await? {
        for edition in event::event_editions_list(&mut conn.mysql_conn, &event.handle).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );

            let mappack = AnyMappackId::Event(&event.event, &edition);

            let _: () = conn.redis_conn.del(mappack_key(mappack)).await?;

            for map in event::event_edition_maps(&mut conn.mysql_conn, edition.event_id, edition.id)
                .await?
            {
                let _: () = conn
                    .redis_conn
                    .sadd(mappack_key(mappack), map.game_id)
                    .await?;
            }

            update_mappack(conn, mappack).await?;
        }
    }

    Ok(())
}

pub async fn update(mut conn: DatabaseConnection) -> anyhow::Result<()> {
    update_event_mappacks(&mut conn).await?;

    let mappacks: Vec<String> = conn.redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(&mut conn, AnyMappackId::Id(&mappack_id)).await?;
    }

    Ok(())
}
