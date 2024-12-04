use std::time::Duration;

use deadpool_redis::redis::AsyncCommands;
use records_lib::{
    acquire,
    context::{Context, Ctx, HasEditionId, HasEventHandle, HasEventId, HasMappackId},
    event,
    mappack::{self, AnyMappackId},
    redis_key::{mappack_key, mappacks_key},
    Database, DatabaseConnection,
};

const PROCESS_DURATION_SECS: u64 = 3600 * 24; // Every day
pub const PROCESS_DURATION: Duration = Duration::from_secs(PROCESS_DURATION_SECS);

#[tracing::instrument(skip(conn, ctx), fields(mappack = %ctx.get_mappack_id().mappack_id()))]
async fn update_mappack<C: HasMappackId>(
    conn: &mut DatabaseConnection<'_>,
    ctx: C,
) -> anyhow::Result<()> {
    let rows = mappack::update_mappack(ctx, conn).await?;
    tracing::info!("Rows: {rows}");
    Ok(())
}

async fn update_event_mappacks(conn: &mut DatabaseConnection<'_>) -> anyhow::Result<()> {
    let ctx = Context::default();
    for event in event::event_list(conn.mysql_conn).await? {
        let mut ctx = ctx.by_ref().with_event_handle(event.handle.as_str());
        for edition in event::event_editions_list(conn.mysql_conn, ctx.get_event_handle()).await? {
            tracing::info!(
                "Got event edition ({}:{}) {:?}",
                edition.event_id,
                edition.id,
                edition.name
            );
            let ctx = ctx.by_ref_mut().with_event_edition(&event.event, &edition);

            let mappack = AnyMappackId::Event(&event.event, &edition);

            let _: () = conn.redis_conn.del(mappack_key(mappack)).await?;

            for map in
                event::event_edition_maps(conn.mysql_conn, ctx.get_event_id(), ctx.get_edition_id())
                    .await?
            {
                let _: () = conn
                    .redis_conn
                    .sadd(mappack_key(mappack), map.game_id)
                    .await?;
            }

            update_mappack(conn, ctx.by_ref().with_mappack(mappack)).await?;
        }
    }

    Ok(())
}

pub async fn update(db: Database) -> anyhow::Result<()> {
    let mut conn = acquire!(db?);

    update_event_mappacks(&mut conn).await?;

    let mappacks: Vec<String> = conn.redis_conn.smembers(mappacks_key()).await?;

    let ctx = Context::default();
    for mappack_id in mappacks {
        update_mappack(
            &mut conn,
            ctx.by_ref().with_mappack(AnyMappackId::Id(&mappack_id)),
        )
        .await?;
    }

    Ok(())
}
