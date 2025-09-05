#[cfg(auth)]
mod with_auth;
#[cfg(auth)]
use with_auth as imp;

#[cfg(not(auth))]
mod without_auth;
#[cfg(not(auth))]
use without_auth as imp;

/// Checks for a successful authentication for the player with its login and ManiaPlanet token.
///
/// # Arguments
///
/// * `_: AuthHeader`: the authentication headers retrieved from the HTTP request.
/// * `required: Role` the required role to be authorized.
///
/// # Returns
///
/// * If the token is invalid, it returns an `Unauthorized` error
/// * If it is valid, but the player doesn't exist in the database, it returns a `PlayerNotFound` error
/// * If the player hasn't the required role, or is banned, it returns a `Forbidden` error
/// * Otherwise, it returns Ok(())
pub use imp::check_auth_for;
