//! Swift's default `JSONEncoder` stores `Date` as a Double of seconds since 2001-01-01 (the Apple reference date).
//! These serde helpers keep our JSON byte-compatible with files written by the Swift app.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serializer};

/// Seconds between the Unix epoch and 2001-01-01T00:00:00Z.
pub const REFERENCE_OFFSET: f64 = 978_307_200.0;

pub fn to_reference(d: &DateTime<Utc>) -> f64 {
    d.timestamp_micros() as f64 / 1_000_000.0 - REFERENCE_OFFSET
}

pub fn from_reference(secs: f64) -> DateTime<Utc> {
    let micros = ((secs + REFERENCE_OFFSET) * 1_000_000.0).round() as i64;
    Utc.timestamp_micros(micros).single().unwrap_or_else(Utc::now)
}

pub fn serialize<S: Serializer>(d: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(to_reference(d))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<DateTime<Utc>, D::Error> {
    Ok(from_reference(f64::deserialize(d)?))
}

pub mod option {
    use super::*;

    pub fn serialize<S: Serializer>(d: &Option<DateTime<Utc>>, s: S) -> Result<S::Ok, S::Error> {
        match d {
            Some(d) => s.serialize_some(&to_reference(d)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<DateTime<Utc>>, D::Error> {
        Ok(Option::<f64>::deserialize(d)?.map(from_reference))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let d = from_reference(780_000_000.5);
        assert!((to_reference(&d) - 780_000_000.5).abs() < 1e-6);
    }
}
