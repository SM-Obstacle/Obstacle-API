mod check;
mod format;
mod parse;

// Used accross the sub-modules
use format::FormattedRequestHead;
use parse::parse_agent;

pub(crate) use check::{flag_invalid_req, is_request_valid};
