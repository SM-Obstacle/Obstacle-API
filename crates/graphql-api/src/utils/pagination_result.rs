use async_graphql::{ID, OutputType, connection};
use sea_orm::{ConnectionTrait, Cursor, SelectorTrait};

use crate::{
    error::GqlResult,
    utils::page_input::{PaginationDirection, PaginationInput},
};

pub(crate) struct PaginationResult<I, T>
where
    T: OutputType,
{
    pub(crate) connection: connection::Connection<ID, T>,
    pub(crate) iter: I,
}

pub(crate) async fn get_paginated<C, S, T, Cur>(
    conn: &C,
    mut query: Cursor<S>,
    pagination_input: &PaginationInput<Cur>,
) -> GqlResult<PaginationResult<impl ExactSizeIterator<Item = <S as SelectorTrait>::Item>, T>>
where
    C: ConnectionTrait,
    T: OutputType,
    S: SelectorTrait,
{
    match &pagination_input.dir {
        PaginationDirection::After { cursor } => {
            let players = query
                .first(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            Ok(PaginationResult {
                connection: connection::Connection::new(
                    cursor.is_some(),
                    players.len() > pagination_input.limit,
                ),
                iter: itertools::Either::Left(players.into_iter().take(pagination_input.limit)),
            })
        }
        PaginationDirection::Before { .. } => {
            let players = query
                .last(pagination_input.limit as u64 + 1)
                .all(conn)
                .await?;
            let amount_to_skip = players.len().saturating_sub(pagination_input.limit);
            Ok(PaginationResult {
                connection: connection::Connection::new(
                    players.len() > pagination_input.limit,
                    true,
                ),
                iter: itertools::Either::Right(players.into_iter().skip(amount_to_skip)),
            })
        }
    }
}
