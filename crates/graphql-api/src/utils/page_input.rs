use async_graphql::connection::CursorType;
use mkenv::prelude::*;
use sea_orm::{Cursor, SelectorTrait, sea_query::IntoValueTuple};

use crate::{
    cursors::{ConnectionParameters, RecordDateCursor},
    error::{ApiGqlError, CursorDecodeErrorKind, GqlResult},
};

pub enum PaginationDirection<C> {
    After { cursor: Option<C> },
    Before { cursor: C },
}

pub struct PaginationInput<C = RecordDateCursor> {
    pub dir: PaginationDirection<C>,
    pub limit: usize,
}

impl<C> PaginationInput<C>
where
    C: CursorType<Error = CursorDecodeErrorKind>,
{
    pub fn try_from_input(input: ConnectionParameters) -> GqlResult<Self> {
        match input {
            ConnectionParameters {
                first,
                after,
                last: None,
                before: None,
            } => {
                let limit = first
                    .map(|t| t.min(crate::config().cursor_max_limit.get()))
                    .unwrap_or(crate::config().cursor_default_limit.get());
                let cursor = match after {
                    Some(after) => {
                        let decoded = C::decode_cursor(&after).map_err(|e| {
                            ApiGqlError::from_cursor_decode_error("after", after.0, e)
                        })?;
                        Some(decoded)
                    }
                    None => None,
                };

                Ok(Self {
                    limit,
                    dir: PaginationDirection::After { cursor },
                })
            }
            ConnectionParameters {
                last,
                before: Some(before),
                after: None,
                first: None,
            } => {
                let limit = last
                    .map(|t| t.min(crate::config().cursor_max_limit.get()))
                    .unwrap_or(crate::config().cursor_default_limit.get());
                let cursor = C::decode_cursor(&before)
                    .map_err(|e| ApiGqlError::from_cursor_decode_error("before", before.0, e))?;
                Ok(Self {
                    limit,
                    dir: PaginationDirection::Before { cursor },
                })
            }
            _ => Err(ApiGqlError::from_pagination_input_error()),
        }
    }
}

pub fn apply_cursor_input<C, S>(cursor: &mut Cursor<S>, input: &PaginationInput<C>)
where
    S: SelectorTrait,
    for<'a> &'a C: IntoValueTuple,
{
    match &input.dir {
        PaginationDirection::After { cursor: after } => {
            cursor.first(input.limit as _);
            if let Some(after) = after {
                cursor.after(after);
            }
        }
        PaginationDirection::Before { cursor: before } => {
            cursor.last(input.limit as _).before(before);
        }
    }
}
