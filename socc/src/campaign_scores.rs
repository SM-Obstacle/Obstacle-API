use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    acquire,
    context::{Context, Ctx, HasDbConnection, HasRedisConnection},
    event,
    mappack::{self, AnyMappackId},
    redis_key::{mappack_key, mappacks_key},
    Database, DatabaseConnection,
};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

#[tracing::instrument(skip(conn))]
async fn update_mappack(
    conn: &mut DatabaseConnection<'_>,
    mappack: AnyMappackId<'_>,
) -> anyhow::Result<()> {
    let rows = mappack::update_mappack(mappack, conn).await?;
    tracing::info!("Rows: {rows}");
    Ok(())
}

async fn update_event_mappacks<'a, C: HasDbConnection<'a>>(mut ctx: C) -> anyhow::Result<()> {
    for event in event::event_list(&mut ctx).await? {
        for edition in event::event_editions_list(&mut ctx, &event.handle).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );
            let mut ctx = ctx.by_ref_mut().with_event_edition(&event.event, &edition);

            let mappack = AnyMappackId::Event(&event.event, &edition);

            let _: () = ctx.get_redis_conn().del(mappack_key(mappack)).await?;

            for map in event::event_edition_maps(&mut ctx).await? {
                let _: () = ctx
                    .get_redis_conn()
                    .sadd(mappack_key(mappack), map.game_id)
                    .await?;
            }

            update_mappack(ctx.get_db_conn(), mappack).await?;
        }
    }

    Ok(())
}

pub async fn update(db: Database) -> anyhow::Result<()> {
    let mut conn = acquire!(db?);
    let mut ctx = Context::default().with_db_conn(&mut conn);

    update_event_mappacks(&mut ctx).await?;

    let mappacks: Vec<String> = ctx.get_redis_conn().smembers(mappacks_key()).await?;

    for mappack_id in mappacks {
        update_mappack(&mut conn, AnyMappackId::Id(&mappack_id)).await?;
    }

    Ok(())
}
