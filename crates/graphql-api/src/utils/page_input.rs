use mkenv::prelude::*;
use sea_orm::SelectorTrait;

use crate::{
    cursors::{ConnectionParameters, expr_tuple::IntoExprTuple, query_builder::CursorQueryBuilder},
    error::{ApiGqlError, GqlResult},
};

pub enum PaginationDirection<C> {
    After { cursor: Option<C> },
    Before { cursor: C },
}

pub struct PaginationInput<C> {
    pub dir: PaginationDirection<C>,
    pub limit: usize,
}

impl<C> PaginationInput<C> {
    pub fn get_cursor(&self) -> Option<&C> {
        match &self.dir {
            PaginationDirection::After { cursor } => cursor.as_ref(),
            PaginationDirection::Before { cursor } => Some(cursor),
        }
    }
}

impl<C> PaginationInput<C> {
    pub fn try_from_input(input: ConnectionParameters<C>) -> GqlResult<Self> {
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
                Ok(Self {
                    limit,
                    dir: PaginationDirection::After { cursor: after },
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
                Ok(Self {
                    limit,
                    dir: PaginationDirection::Before { cursor: before },
                })
            }
            _ => Err(ApiGqlError::from_pagination_input_error()),
        }
    }
}

pub fn apply_cursor_input<C, S>(cursor: &mut CursorQueryBuilder<S>, input: &PaginationInput<C>)
where
    S: SelectorTrait,
    for<'a> &'a C: IntoExprTuple,
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
