mod queryroot_maps_connection;
mod queryroot_players_connection;
mod queryroot_records_connection;

#[tokio::test]
async fn player_and_map_records_amount() -> anyhow::Result<()> {
    test_env::wrap(async |db| {
        use chrono::TimeZone as _;
        use sea_orm::{ActiveValue::Set, EntityTrait as _};

        entity::players::Entity::insert(entity::players::ActiveModel {
            id: Set(1),
            login: Set("player_login".to_owned()),
            name: Set("player_name".to_owned()),
            join_date: Set(Some(
                chrono::Utc
                    .with_ymd_and_hms(2026, 11, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc(),
            )),
            role: Set(0),
            ..Default::default()
        })
        .exec(&db.sql_conn)
        .await?;

        entity::maps::Entity::insert(entity::maps::ActiveModel {
            id: Set(1),
            game_id: Set("map_uid".to_owned()),
            player_id: Set(1),
            name: Set("map_name".to_owned()),
            ..Default::default()
        })
        .exec(&db.sql_conn)
        .await?;

        entity::records::Entity::insert_many([
            entity::records::ActiveModel {
                record_id: Set(1),
                record_player_id: Set(1),
                map_id: Set(1),
                time: Set(100),
                respawn_count: Set(0),
                record_date: Set(chrono::Utc
                    .with_ymd_and_hms(2026, 11, 22, 12, 0, 0)
                    .unwrap()
                    .naive_utc()),
                flags: Set(0),
                ..Default::default()
            },
            entity::records::ActiveModel {
                record_id: Set(2),
                record_player_id: Set(1),
                map_id: Set(1),
                time: Set(120),
                respawn_count: Set(0),
                record_date: Set(chrono::Utc
                    .with_ymd_and_hms(2026, 11, 22, 13, 0, 0)
                    .unwrap()
                    .naive_utc()),
                flags: Set(0),
                ..Default::default()
            },
        ])
        .exec(&db.sql_conn)
        .await?;

        assert_eq!(
            crate::objects::player::get_player_records_amount(&db.sql_conn, 1).await?,
            1
        );
        assert_eq!(
            crate::objects::map::get_map_records_amount(&db.sql_conn, 1).await?,
            1
        );

        anyhow::Ok(())
    })
    .await
}

mod maps_records_connection;
mod players_records_connection;
mod records_connection_ranks;
