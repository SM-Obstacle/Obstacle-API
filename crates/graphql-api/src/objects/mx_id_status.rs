/// What the API knows about the MX ID of a map.
///
/// This tells apart a map ManiaExchange doesn't have from one we simply haven't got an answer for
/// yet, which both leave `mxId` null.
#[derive(async_graphql::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MxIdStatus {
    /// We have the MX ID of this map: the `mxId` field holds it.
    Known,
    /// ManiaExchange doesn't have this map. It's worth asking again in a while, but not now.
    NotOnMx,
    /// We have no answer for this map yet. It's being fetched, so `mxId` may be filled in a few
    /// seconds; asking again later, or asking for it explicitly, is what tells.
    Unknown,
}

impl From<mx_layer::maps::MxIdStatus> for MxIdStatus {
    fn from(status: mx_layer::maps::MxIdStatus) -> Self {
        match status {
            mx_layer::maps::MxIdStatus::Known(_) => Self::Known,
            mx_layer::maps::MxIdStatus::NotOnMx => Self::NotOnMx,
            mx_layer::maps::MxIdStatus::Unknown => Self::Unknown,
        }
    }
}
