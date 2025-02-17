use std::str::FromStr;

use nom::{
    bytes::{tag, take_until, take_while1},
    combinator::map_res,
    Parser as _,
};

pub(super) struct ParsedAgent<'a> {
    pub(super) maniaplanet_version: (u8, u8, u8),
    pub(super) rv: (u16, u8, u8, u8, u8),
    pub(super) client: &'a [u8],
    pub(super) context: &'a [u8],
    pub(super) _distro: &'a [u8],
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

pub(super) fn parse_agent(input: &[u8]) -> nom::IResult<&[u8], ParsedAgent> {
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
    let (input, (_, rv)) = (tag(" rv: "), take_until(";")).parse(input)?;
    let (_, rv) = parse_rv(rv)?;
    let (input, (_, context)) = (tag(" context: "), take_until(";")).parse(input)?;
    let (input, (_, distro)) = (tag(" distro: "), take_until(";")).parse(input)?;

    Ok((
        input,
        ParsedAgent {
            maniaplanet_version: (major, minor, patch),
            rv,
            client,
            context,
            _distro: distro,
        },
    ))
}
