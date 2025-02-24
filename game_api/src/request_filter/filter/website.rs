use nom::{Parser, bytes::complete::tag};

pub struct WebsiteAgent;

pub struct ParseError;

fn parse_agent(input: &[u8]) -> nom::IResult<&[u8], WebsiteAgent> {
    let (input, _) = tag("node").parse(input)?;
    Ok((input, WebsiteAgent))
}

impl super::FromBytes for WebsiteAgent {
    type Error = ParseError;

    fn from_bytes(b: &[u8]) -> Result<Self, Self::Error> {
        parse_agent(b).map_err(|_| ParseError).map(|(_, o)| o)
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
