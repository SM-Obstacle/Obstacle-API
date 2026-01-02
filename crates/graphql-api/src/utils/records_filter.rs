use entity::{global_event_records, global_records, maps, players, records};
use sea_orm::{
    ColumnTrait, EntityTrait, JoinType, QueryFilter as _, QuerySelect as _, RelationDef,
    RelationTrait as _, Select,
    prelude::Expr,
    sea_query::{ExprTrait as _, Func},
};

use crate::objects::records_filter::RecordsFilter;

pub trait RecordsTableFilterConstructor {
    type Column: ColumnTrait;

    const COL_RECORD_DATE: Self::Column;
    const COL_TIME: Self::Column;

    fn get_players_relation() -> RelationDef;

    fn get_maps_relation() -> RelationDef;
}

impl RecordsTableFilterConstructor for global_records::Entity {
    type Column = global_records::Column;

    const COL_RECORD_DATE: Self::Column = global_records::Column::RecordDate;
    const COL_TIME: Self::Column = global_records::Column::Time;

    fn get_players_relation() -> RelationDef {
        global_records::Relation::Players.def()
    }

    fn get_maps_relation() -> RelationDef {
        global_records::Relation::Maps.def()
    }
}

impl RecordsTableFilterConstructor for records::Entity {
    type Column = records::Column;

    const COL_RECORD_DATE: Self::Column = records::Column::RecordDate;

    const COL_TIME: Self::Column = records::Column::Time;

    fn get_players_relation() -> RelationDef {
        records::Relation::Players.def()
    }

    fn get_maps_relation() -> RelationDef {
        records::Relation::Maps.def()
    }
}

impl RecordsTableFilterConstructor for global_event_records::Entity {
    type Column = global_event_records::Column;

    const COL_RECORD_DATE: Self::Column = global_event_records::Column::RecordDate;

    const COL_TIME: Self::Column = global_event_records::Column::Time;

    fn get_players_relation() -> RelationDef {
        global_event_records::Relation::Players.def()
    }

    fn get_maps_relation() -> RelationDef {
        global_event_records::Relation::Maps.def()
    }
}

pub fn apply_filter<E>(mut query: Select<E>, filter: Option<&RecordsFilter>) -> Select<E>
where
    E: RecordsTableFilterConstructor + EntityTrait,
{
    let Some(filter) = filter else {
        return query;
    };

    // Join with players table if needed for player filters
    if filter.player.is_some() {
        query = query.join_as(JoinType::InnerJoin, E::get_players_relation(), "p");
    }

    // Join with maps table if needed for map filters
    if let Some(m) = &filter.map {
        query = query.join_as(JoinType::InnerJoin, E::get_maps_relation(), "m");

        // Join again with players table if filtering on map author
        if m.author.is_some() {
            query = query.join_as(JoinType::InnerJoin, maps::Relation::Players.def(), "p2");
        }
    }

    if let Some(filter) = &filter.player {
        // Apply player login filter
        if let Some(login) = &filter.player_login {
            query =
                query.filter(Expr::col(("p", players::Column::Login)).like(format!("%{login}%")));
        }

        // Apply player name filter
        if let Some(name) = &filter.player_name {
            query = query.filter(
                Func::cust("rm_mp_style")
                    .arg(Expr::col(("p", players::Column::Name)))
                    .like(format!("%{name}%")),
            );
        }
    }

    if let Some(filter) = &filter.map {
        // Apply map UID filter
        if let Some(uid) = &filter.map_uid {
            query = query.filter(Expr::col(("m", maps::Column::GameId)).like(format!("%{uid}%")));
        }

        // Apply map name filter
        if let Some(name) = &filter.map_name {
            query = query.filter(
                Func::cust("rm_mp_style")
                    .arg(Expr::col(("m", maps::Column::Name)))
                    .like(format!("%{name}%")),
            );
        }

        if let Some(filter) = &filter.author {
            // Apply player login filter
            if let Some(login) = &filter.player_login {
                query = query
                    .filter(Expr::col(("p2", players::Column::Login)).like(format!("%{login}%")));
            }

            // Apply player name filter
            if let Some(name) = &filter.player_name {
                query = query.filter(
                    Func::cust("rm_mp_style")
                        .arg(Expr::col(("p2", players::Column::Name)))
                        .like(format!("%{name}%")),
                );
            }
        }
    }

    // Apply date filters
    if let Some(before_date) = filter.before_date {
        query = query.filter(E::COL_RECORD_DATE.lt(before_date));
    }

    if let Some(after_date) = filter.after_date {
        query = query.filter(E::COL_RECORD_DATE.gt(after_date));
    }

    // Apply time filters
    if let Some(time_gt) = filter.time_gt {
        query = query.filter(E::COL_TIME.gt(time_gt));
    }

    if let Some(time_lt) = filter.time_lt {
        query = query.filter(E::COL_TIME.lt(time_lt));
    }

    query
}
