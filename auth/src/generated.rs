use std::fmt::{self, Write as _};

use rand::Rng;
use secure_string::SecureString;
use sha2::Digest;

type Hasher = sha2::Sha224;
type HasherOutputSize = <Hasher as sha2::digest::OutputSizeUser>::OutputSize;
pub(crate) type CodeHash = [u8; <HasherOutputSize as sha2::digest::typenum::Unsigned>::USIZE];

const STATE_ID_LEN: usize = 20;
const TOKEN_LEN: usize = 256;
const CODE_LEN: usize = 10;

/// The type of an authentication token.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Token {
    s: SecureString,
}

fn generate_str(len: usize) -> SecureString {
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .map(char::from)
        .take(len)
        .collect::<String>()
        .into()
}

impl Token {
    pub(crate) fn new() -> Self {
        Self {
            s: generate_str(TOKEN_LEN),
        }
    }
}

/// The type of the code shown to the player in their browser.
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Code {
    s: SecureString,
}

impl From<&str> for Code {
    fn from(value: &str) -> Self {
        Self { s: value.into() }
    }
}

impl Code {
    pub(crate) fn new() -> Self {
        Self {
            s: generate_str(CODE_LEN),
        }
    }

    pub(crate) fn hash_with(&self, state_id: StateId) -> CodeHash {
        Hasher::new()
            .chain_update(self.s.unsecure().as_bytes())
            .chain_update(state_id)
            .finalize()
            .into()
    }
}

/// The type of authentication state ID.
#[derive(serde::Deserialize, serde::Serialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
#[repr(transparent)]
pub struct StateId([u8; STATE_ID_LEN]);

impl StateId {
    pub(crate) fn new() -> Self {
        let mut rng = rand::thread_rng();
        Self([0; STATE_ID_LEN].map(|_| rng.sample(rand::distributions::Alphanumeric)))
    }

    #[cfg(test)]
    pub(crate) fn dummy() -> Self {
        Self([0; STATE_ID_LEN])
    }
}

#[derive(thiserror::Error, Debug)]
#[error("invalid state id length")]
pub struct InvalidLength;

impl TryFrom<String> for StateId {
    type Error = InvalidLength;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.into_bytes().try_into() {
            Ok(inner) => Ok(Self(inner)),
            Err(_) => Err(InvalidLength),
        }
    }
}

impl From<StateId> for String {
    #[inline(always)]
    fn from(state_id: StateId) -> String {
        state_id.to_string()
    }
}

impl AsRef<[u8]> for StateId {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        AsRef::as_ref(&self.0)
    }
}

impl fmt::Display for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            f.write_char(b as _)?;
        }
        Ok(())
    }
}

impl fmt::Debug for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
