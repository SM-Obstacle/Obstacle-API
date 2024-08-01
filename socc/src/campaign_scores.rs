use std::time::Duration;

use deadpool_redis::{redis::AsyncCommands, Connection};
use records_lib::{
    event,
    mappack::{update_mappack, AnyMappackId},
    redis_key::{mappack_key, mappacks_key},
    DatabaseConnection,
};
use sqlx::{pool::PoolConnection, MySql};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

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

            conn.redis_conn.del(mappack_key(mappack)).await?;

            for map in event::event_edition_maps(&mut conn.mysql_conn, edition.event_id, edition.id)
                .await?
            {
                conn.redis_conn
                    .sadd(mappack_key(mappack), map.game_id)
                    .await?;
            }

            update_mappack(mappack, conn).await?;
        }
    }

    Ok(())
}

pub async fn update(
    mysql_conn: PoolConnection<MySql>,
    redis_conn: Connection,
) -> anyhow::Result<()> {
    let mut conn = DatabaseConnection {
        mysql_conn,
        redis_conn,
    };

    update_event_mappacks(&mut conn).await?;

    let mappacks: Vec<String> = conn.redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(AnyMappackId::Id(&mappack_id), &mut conn).await?;
    }

    Ok(())
}
