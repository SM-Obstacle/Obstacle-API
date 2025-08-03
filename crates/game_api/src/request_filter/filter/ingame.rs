use std::str::FromStr;

use nom::{
    Parser as _,
    bytes::complete::{tag, take_until, take_while1},
    combinator::map_res,
};

#[derive(Clone)]
pub struct InGameAgent {
    pub maniaplanet_version: (u8, u8, u8),
    pub rv: (u16, u8, u8, u8, u8),
    pub client: Box<[u8]>,
    pub context: Box<[u8]>,
    pub _distro: Box<[u8]>,
}

fn parse_unsigned<T: FromStr>(input: &[u8]) -> nom::IResult<&[u8], T> {
    map_res(
        take_while1(|c: u8| c.is_ascii_digit()),
        |input| match std::str::from_utf8(input) {
            Ok(s) => s.parse().map_err(|_| ()),
            Err(_) => Err(()),
        },
    )
    .parse(input)
}

fn parse_rv(input: &[u8]) -> nom::IResult<&[u8], (u16, u8, u8, u8, u8)> {
    let (input, (a, _, b, _, c, _, d, _, e)) = (
        parse_unsigned,
        tag("-"),
        parse_unsigned,
        tag("-"),
        parse_unsigned,
        tag("_"),
        parse_unsigned,
        tag("_"),
        parse_unsigned,
    )
        .parse(input)?;
    Ok((input, (a, b, c, d, e)))
}

fn parse_agent(input: &[u8]) -> nom::IResult<&[u8], InGameAgent> {
    let (input, _) = tag("ManiaPlanet/").parse(input)?;
    let (input, (major, _, minor, _, patch)) = (
        parse_unsigned,
        tag("."),
        parse_unsigned,
        tag("."),
        parse_unsigned,
    )
        .parse(input)?;
    let (input, (_, client)) = (tag(" ("), take_until(";")).parse(input)?;
    let (input, (_, rv)) = (tag("; rv: "), take_until(";")).parse(input)?;
    let (_, rv) = parse_rv(rv)?;
    let (input, (_, context)) = (tag("; context: "), take_until(";")).parse(input)?;
    let (input, (_, distro)) = (tag("; distro: "), take_until(")")).parse(input)?;

    Ok((
        input,
        InGameAgent {
            maniaplanet_version: (major, minor, patch),
            rv,
            client: Box::from(client),
            context: Box::from(context),
            _distro: Box::from(distro),
        },
    ))
}

pub struct ParseError;

impl TryFrom<&[u8]> for InGameAgent {
    type Error = ParseError;

    fn try_from(input: &[u8]) -> Result<Self, Self::Error> {
        parse_agent(input)
            .map_err(|_| ParseError)
            .map(|(_, out)| out)
    }
}

pub struct InGameFilter;

impl super::FilterAgent for InGameFilter {
    type AgentType = InGameAgent;

    fn is_valid(parsed: &Self::AgentType) -> bool {
        match &*parsed.client {
            b"Win64" | b"Win32" | b"Linux" => (),
            _ => return false,
        }

        match parsed.maniaplanet_version {
            // Please let me know when Nadeo releases a new version of ManiaPlanet... :(
            (..3, ..) | (3, ..3, _) | (3, 3, 0) => (),
            _ => return false,
        }

        match parsed.rv {
            (..2019, ..)
            | (2019, ..11, ..)
            | (2019, 11, ..19, ..)
            | (2019, 11, 19, ..18, _)
            | (2019, 11, 19, 18, ..=58) => (),
            _ => return false,
        }

        match &*parsed.context {
            b"none" => (),
            _ => return false,
        }

        true
    }
}
