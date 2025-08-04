#[derive(Clone)]
pub struct WebsiteAgent;

pub struct ParseError;

impl TryFrom<&[u8]> for WebsiteAgent {
    type Error = ParseError;

    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        if b == b"node" {
            Ok(Self)
        } else {
            Err(ParseError)
        }
    }
}

pub struct WebsiteFilter;

impl super::FilterAgent for WebsiteFilter {
    type AgentType = WebsiteAgent;

    #[inline(always)]
    fn is_valid(_: &Self::AgentType) -> bool {
        true
    }
}
