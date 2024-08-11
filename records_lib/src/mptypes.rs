use serde::ser::{Serialize, Serializer};

/// The Integer type in ManiaScript.
pub type Integer = i32;
/// The Real type in ManiaScript.
pub type Real = f64;
/// The Text type in ManiaScript.
pub type Text = String;

/// A ManiaScript Integer that can be null.
///
/// If the value is null, the serialized value is `DEFAULT` (`-1` by default).
///
/// This is used to serialize structures into JSON so that they can be deserialized correctly
/// in ManiaScript.
///
/// The reason of this is because null values in JSON aren't valid in ManiaScript, so their null equivalent
/// are the default values.
#[derive(Default)]
pub struct NullableInteger<const DEFAULT: Integer = -1>(pub Option<Integer>);

impl<const DEFAULT: Integer> From<Option<Integer>> for NullableInteger<DEFAULT> {
    fn from(d: Option<Integer>) -> Self {
        Self(d)
    }
}

impl<'r, DB: sqlx::Database> sqlx::Decode<'r, DB> for NullableInteger
where
    Option<Integer>: sqlx::Decode<'r, DB>,
{
    #[inline]
    fn decode(
        value: <DB as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <Option<Integer> as sqlx::Decode<'r, DB>>::decode(value).map(From::from)
    }
}

impl<DB: sqlx::Database> sqlx::Type<DB> for NullableInteger
where
    Option<Integer>: sqlx::Type<DB>,
{
    fn type_info() -> <DB as sqlx::Database>::TypeInfo {
        <Option<Integer> as sqlx::Type<DB>>::type_info()
    }

    fn compatible(ty: &<DB as sqlx::Database>::TypeInfo) -> bool {
        <Option<Integer> as sqlx::Type<DB>>::compatible(ty)
    }
}

impl<const DEFAULT: Integer> Serialize for NullableInteger<DEFAULT> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(i) => i.serialize(s),
            None => s.serialize_i32(DEFAULT),
        }
    }
}

/// A ManiaScript Real that can be null.
///
/// If the value is null, the serialized value is `-1.0`.
///
/// This is used to serialize structures into JSON so that they can be deserialized correctly
/// in ManiaScript.
///
/// The reason of this is because null values in JSON aren't valid in ManiaScript, so their null equivalent
/// are the default values.
#[derive(Default)]
pub struct NullableReal(pub Option<Real>);

impl From<Option<Real>> for NullableReal {
    fn from(f: Option<Real>) -> Self {
        Self(f)
    }
}

impl<'r, DB: sqlx::Database> sqlx::Decode<'r, DB> for NullableReal
where
    Option<Real>: sqlx::Decode<'r, DB>,
{
    #[inline]
    fn decode(
        value: <DB as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <Option<Real> as sqlx::Decode<'r, DB>>::decode(value).map(From::from)
    }
}

impl<DB: sqlx::Database> sqlx::Type<DB> for NullableReal
where
    Option<Real>: sqlx::Type<DB>,
{
    fn type_info() -> <DB as sqlx::Database>::TypeInfo {
        <Option<Real> as sqlx::Type<DB>>::type_info()
    }

    fn compatible(ty: &<DB as sqlx::Database>::TypeInfo) -> bool {
        <Option<Real> as sqlx::Type<DB>>::compatible(ty)
    }
}

impl Serialize for NullableReal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(i) => i.serialize(s),
            None => s.serialize_f64(-1.),
        }
    }
}

/// A ManiaScript Text that can be null.
///
/// If the value is null, the serialized value is `""`.
///
/// This is used to serialize structures into JSON so that they can be deserialized correctly
/// in ManiaScript.
///
/// The reason of this is because null values in JSON aren't valid in ManiaScript, so their null equivalent
/// are the default values.
#[derive(Default)]
pub struct NullableText(pub Option<Text>);

impl From<Option<Text>> for NullableText {
    fn from(t: Option<Text>) -> Self {
        Self(t)
    }
}

impl<'r, DB: sqlx::Database> sqlx::Decode<'r, DB> for NullableText
where
    Option<Text>: sqlx::Decode<'r, DB>,
{
    #[inline]
    fn decode(
        value: <DB as sqlx::Database>::ValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        <Option<Text> as sqlx::Decode<'r, DB>>::decode(value).map(From::from)
    }
}

impl<DB: sqlx::Database> sqlx::Type<DB> for NullableText
where
    Option<Text>: sqlx::Type<DB>,
{
    fn type_info() -> <DB as sqlx::Database>::TypeInfo {
        <Option<Text> as sqlx::Type<DB>>::type_info()
    }

    fn compatible(ty: &<DB as sqlx::Database>::TypeInfo) -> bool {
        <Option<Text> as sqlx::Type<DB>>::compatible(ty)
    }
}

impl Serialize for NullableText {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(ref i) => i.serialize(s),
            None => s.serialize_str(""),
        }
    }
}
