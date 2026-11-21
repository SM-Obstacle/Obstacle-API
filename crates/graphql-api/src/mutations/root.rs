use mx_layer::maps::MxIdProvider;
use sea_orm::DbConn;

use crate::{
    error::{ApiGqlError, GqlResult},
    objects::map::Map,
    utils::force_fetch::ForceFetchBudget,
};

pub struct MutationRoot;

#[async_graphql::Object]
impl MutationRoot {
    /// Asks the ManiaExchange API for the MX ID of a map right now, instead of leaving it to be
    /// fetched in the background, and saves it in our database.
    ///
    /// This is what a "look for this map on ManiaExchange" button calls: it ignores what we may
    /// have cached about this map being absent from ManiaExchange, which is the whole point, since
    /// whoever presses that button knows better than our cache does.
    ///
    /// It costs a request to the ManiaExchange API, so **it can only be used once per mutation**.
    /// A map whose MX ID we already have doesn't count, since it needs no request.
    ///
    /// Returns the map, whose `mxId` is null when ManiaExchange doesn't have it.
    async fn force_fetch_mx_id(
        &self,
        ctx: &async_graphql::Context<'_>,
        #[graphql(desc = "The UID of the map, as the game knows it.")] map_uid: String,
    ) -> GqlResult<Map> {
        let conn = ctx.data_unchecked::<DbConn>();

        // Looking the map up first also keeps anybody from having us ask MX about anything at all.
        let mut map = records_lib::map::get_map_from_uid(conn, &map_uid)
            .await?
            .ok_or_else(|| ApiGqlError::from_map_not_found_error(map_uid.clone()))?;

        // There's nothing to fetch if our database already has it.
        if map.mx_id.is_some() {
            return Ok(map.into());
        }

        if !ctx.data_unchecked::<ForceFetchBudget>().spend() {
            return Err(ApiGqlError::from_mx_id_force_fetch_limit());
        }

        // The MX ID is saved by the provider, but only once whoever asked for it got it: the map
        // we return holds it right away, instead of the null our row still has.
        map.mx_id = ctx
            .data_unchecked::<MxIdProvider>()
            .force_fetch(&map_uid)
            .await?;

        Ok(map.into())
    }
}
