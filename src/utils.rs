use std::{fmt, str::FromStr};

use serde::{de, ser, Deserialize, Serialize};

pub fn serialize_as_string<N, S>(value: &N, serializer: S) -> Result<S::Ok, S::Error>
where
    N: Serialize + Copy + ToString + FromStr,
    S: ser::Serializer,
{
    String::serialize(&value.to_string(), serializer)
}

pub fn deserialize_as_string<'de, N, D>(deserializer: D) -> Result<N, D::Error>
where
    N: Deserialize<'de> + Copy + ToString + FromStr,
    D: de::Deserializer<'de>,
    <N as FromStr>::Err: fmt::Display,
{
    let s = String::deserialize(deserializer)?;
    Ok(s.parse::<N>().map_err(de::Error::custom)?)
}
