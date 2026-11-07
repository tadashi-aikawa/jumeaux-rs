//! OwlMixinは欠落と明示nullの両方でフィールドの既定値を使う。
use serde::{Deserialize, Deserializer};

pub(crate) fn null_default<'de, T: Deserialize<'de> + Default, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

macro_rules! null_default_with {
    ($name:ident, $ty:ty, $default:expr) => {
        fn $name<'de, D: serde::Deserializer<'de>>(
            deserializer: D,
        ) -> std::result::Result<$ty, D::Error> {
            Ok(Option::<$ty>::deserialize(deserializer)?.unwrap_or_else(|| $default))
        }
    };
}
pub(crate) use null_default_with;
