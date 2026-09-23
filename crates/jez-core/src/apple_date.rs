//! Swift's default `JSONEncoder` stores `Date` as a Double of seconds since 2001-01-01 (the Apple reference date).
//! These serde helpers keep our JSON byte-compatible with files written by the Swift app.

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Deserializer, Serializer};

/// Seconds between the Unix epoch and 2001-01-01T00:00:00Z.
pub const REFERENCE_OFFSET: f64 = 978_307_200.0;

/// Whole seconds and the nanosecond fraction are converted separately, so a Double read from a Swift file
/// converts back to the identical Double (microsecond rounding used to change the last digits on re-save).
pub fn to_reference(d: &DateTime<Utc>) -> f64 {
    (d.timestamp() - REFERENCE_OFFSET as i64) as f64 + d.timestamp_subsec_nanos() as f64 / 1e9
}

pub fn from_reference(secs: f64) -> DateTime<Utc> {
    if !secs.is_finite() {
        return Utc::now();
    }
    let whole = secs.floor();
    let mut nanos = ((secs - whole) * 1e9).round() as i64;
    let mut whole = whole as i64;
    if nanos >= 1_000_000_000 {
        whole += 1;
        nanos -= 1_000_000_000;
    }
    Utc.timestamp_opt(whole + REFERENCE_OFFSET as i64, nanos as u32).single().unwrap_or_else(Utc::now)
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

    #[test]
    fn round_trips_bit_exact() {
        // Typical `Date()` values written by Swift, plus pre-2001 and integral ones.
        let mut x = 780_000_000.123_456_7_f64;
        for i in 0..10_000 {
            x += 0.000_137 * i as f64 + 1.0 / 3.0;
            for v in [x, -x, (x * 1000.0).round() / 1000.0, x.floor(), 0.0, 5.0e-7] {
                assert_eq!(to_reference(&from_reference(v)).to_bits(), v.to_bits(), "{v}");
            }
        }
    }

    #[test]
    fn unix_offset() {
        assert_eq!(from_reference(0.0).timestamp(), 978_307_200);
        assert_eq!(from_reference(-0.25).timestamp_subsec_millis(), 750);
    }
}
