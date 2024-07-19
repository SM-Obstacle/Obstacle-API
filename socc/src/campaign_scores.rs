use std::time::Duration;

use deadpool_redis::{redis::AsyncCommands, Connection};
use records_lib::{
    event,
    mappack::{update_mappack, AnyMappackId},
    redis_key::{mappack_key, mappacks_key},
};
use sqlx::{pool::PoolConnection, MySql, MySqlConnection};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

async fn update_event_mappacks(
    mysql_conn: &mut MySqlConnection,
    redis_conn: &mut Connection,
) -> anyhow::Result<()> {
    for event in event::event_list(mysql_conn).await? {
        for edition in event::event_editions_list(mysql_conn, &event.handle).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );

            let mappack = AnyMappackId::Event(&event.event, &edition);

            redis_conn.del(mappack_key(mappack)).await?;

            for map in event::event_edition_maps(mysql_conn, edition.event_id, edition.id).await? {
                redis_conn.sadd(mappack_key(mappack), map.game_id).await?;
            }

            update_mappack(mappack, mysql_conn, redis_conn).await?;
        }
    }

    Ok(())
}

pub async fn update(
    mut mysql_conn: PoolConnection<MySql>,
    mut redis_conn: Connection,
) -> anyhow::Result<()> {
    update_event_mappacks(&mut mysql_conn, &mut redis_conn).await?;

    let mappacks: Vec<String> = redis_conn.smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(
            AnyMappackId::Id(&mappack_id),
            &mut mysql_conn,
            &mut redis_conn,
        )
        .await?;
    }

    Ok(())
}
