use records_lib::{must, MySqlPool};
use sqlx::MySqlConnection;

#[derive(clap::Args)]
pub struct ClearCommand {
    event_handle: String,
    event_edition: u32,
}

#[tracing::instrument(skip(db))]
pub async fn clear_content(
    db: &mut MySqlConnection,
    event_id: u32,
    edition_id: u32,
) -> anyhow::Result<()> {
    sqlx::query("delete from event_edition_maps where event_id = ? and edition_id = ?")
        .bind(event_id)
        .bind(edition_id)
        .execute(&mut *db)
        .await?;

    Ok(())
}

pub async fn clear(
    db: MySqlPool,
    ClearCommand {
        event_handle,
        event_edition,
    }: ClearCommand,
) -> anyhow::Result<()> {
    let mysql_conn = &mut db.acquire().await?;

    let (event, edition) =
        must::have_event_edition(mysql_conn, &event_handle, event_edition).await?;

    clear_content(mysql_conn, event.id, edition.id).await?;

    Ok(())
}
