//! Helpers for the suspend point (`giana_core::snapshot`): serde cannot do arrays longer than 32 by itself.

/// `#[serde(with = "crate::snap::arr")]` for a `[T; N]` of any length: stored as a sequence.
pub mod arr {
    use serde::de::Error;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(a: &[T; N], s: S) -> Result<S::Ok, S::Error> {
        a.as_slice().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>, const N: usize>(d: D) -> Result<[T; N], D::Error> {
        let v = Vec::<T>::deserialize(d)?;
        let n = v.len();
        v.try_into().map_err(|_| D::Error::invalid_length(n, &"an array of the right length"))
    }
}
