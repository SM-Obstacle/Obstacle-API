/// Specialization of the [`Default`] trait for ManiaScript.
///
/// The use of this trait results in the fact that in ManiaScript, the JSON parsing of null fields
/// fails. So we need to provide the default value of the type if the field is null ([`None`]).
///
/// Optional strings result to `""` and optional integers result to `-1`.
pub trait MpDefault {
    /// Returns the default value of the type, for use in ManiaScript later.
    fn mp_default() -> Self;
}

/// Allows the special case for default int values for the [`MpDefault`] trait.
#[derive(serde::Serialize)]
#[repr(transparent)]
pub struct MpDefaultI32<const DEFAULT: i32 = -1>(pub i32);

impl<'r, const DEFAULT: i32, Db> sqlx::Decode<'r, Db> for MpDefaultI32<DEFAULT>
where
    i32: sqlx::Decode<'r, Db>,
    Db: sqlx::Database,
{
    #[inline]
    fn decode(
        value: <Db as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <i32 as sqlx::Decode<'r, Db>>::decode(value).map(From::from)
    }
}

impl<Db, const DEFAULT: i32> sqlx::Type<Db> for MpDefaultI32<DEFAULT>
where
    Db: sqlx::Database,
    i32: sqlx::Type<Db>,
{
    #[inline]
    fn type_info() -> <Db as sqlx::Database>::TypeInfo {
        <i32 as sqlx::Type<Db>>::type_info()
    }

    #[inline]
    fn compatible(ty: &<Db as sqlx::Database>::TypeInfo) -> bool {
        <i32 as sqlx::Type<Db>>::compatible(ty)
    }
}

macro_rules! from_impls {
    ($($t:ty)*) => {$(
        impl<const DEFAULT: i32> From<$t> for MpDefaultI32<DEFAULT> {
            #[inline]
            fn from(value: $t) -> Self {
                Self(value as _)
            }
        }
    )*};
}

from_impls!(u8 u16 u32 u64 u128 i8 i16 i32 i64 i128);

impl<const DEFAULT: i32> MpDefault for MpDefaultI32<DEFAULT> {
    #[inline]
    fn mp_default() -> Self {
        Self(DEFAULT)
    }
}

impl<T: Default> MpDefault for T {
    #[inline]
    fn mp_default() -> Self {
        Default::default()
    }
}

/// Serializes an optional value to its default value if not present for use in ManiaScript later.
///
/// See the [`MpDefault`] trait documentation for more information.
pub fn opt_ser<T, S>(obj: &Option<T>, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
    T: serde::Serialize + MpDefault,
{
    let obj = match obj {
        Some(o) => o,
        None => &MpDefault::mp_default(),
    };
    obj.serialize(ser)
}
