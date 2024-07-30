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
pub struct MpDefaultI32<const DEFAULT: i32 = -1>(pub i32);

impl<'r, const DEFAULT: i32, Db> sqlx::Decode<'r, Db> for MpDefaultI32<DEFAULT>
where
    i32: sqlx::Decode<'r, Db>,
    Db: sqlx::Database,
{
    #[inline]
    fn decode(
        value: <Db as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        Ok(Self(<i32>::decode(value)?))
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

impl<const DEFAULT: i32> From<i32> for MpDefaultI32<DEFAULT> {
    #[inline]
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl<const DEFAULT: i32> MpDefault for MpDefaultI32<DEFAULT> {
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
