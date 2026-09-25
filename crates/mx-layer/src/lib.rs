//! Everything related to [ManiaExchange].
//!
//! [ManiaExchange]: https://sm.mania.exchange
//!
//! MX is an API that can be slow, down, and shouldn't be flooded. Therefore, only this crate talks
//! to it.

pub mod api;
pub mod mappacks;
pub mod maps;
pub mod polite;

#[cfg(test)]
mod fakes;

use std::sync::Arc;

use mappacks::MappackProvider;
use maps::{MxIdProvider, MxIdSink, MxMapProvider};

/// Every way the Obstacle API asks MX something.
#[derive(Clone)]
pub struct MxLayer {
    /// The MX ID of the maps, from their UID.
    pub map_mx_ids: MxIdProvider,
    /// The maps of MX, from their MX ID.
    pub mx_maps: MxMapProvider,
    /// The maps and the description of the mappacks.
    pub mappacks: MappackProvider,
}

impl MxLayer {
    /// Builds the layer on top of the provided HTTP client, and spawns the workers batching the
    /// requests to MX.
    ///
    /// The sink is where the MX IDs we fetch are saved; the tools which have nowhere to save them
    /// pass `()`.
    ///
    /// This must be called from within a Tokio runtime.
    pub fn new<S: MxIdSink>(client: reqwest::Client, mx_id_sink: S) -> Self {
        let sink = Arc::new(mx_id_sink);
        Self {
            map_mx_ids: MxIdProvider::from_client(client.clone(), Arc::clone(&sink)),
            mx_maps: MxMapProvider::from_client(client.clone()),
            mappacks: MappackProvider::from_client(client, sink),
        }
    }
}
