//! Protocol encoding/1 on the client's side: the **strict** parse (the
//! value domain the service accepts: no floats, no integer past 2^53 - 1,
//! no noncharacter, no duplicate key) and the canonical form a command's
//! intent is digested in.
//!
//! The service has its own implementation (`pio_protocol`'s, private to it).
//! This one is the client's, so a caller that must hold exactly what the
//! service will read (the caller ledger) parses as strictly as the service
//! does without reaching into the service's crate. Its tests pin the same
//! refusals.
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::fmt;

struct Strict(Value);
fn scalar(s: &str) -> bool {
    s.chars().all(|c| {
        let n = c as u32;
        !(0xfdd0..=0xfdef).contains(&n) && n & 0xffff != 0xfffe && n & 0xffff != 0xffff
    })
}
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("encoding/1 JSON")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Strict, E> {
                if v.unsigned_abs() > 9007199254740991 {
                    Err(E::custom("unsafe integer"))
                } else {
                    Ok(Strict(v.into()))
                }
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Strict, E> {
                if v > 9007199254740991 {
                    Err(E::custom("unsafe integer"))
                } else {
                    Ok(Strict(v.into()))
                }
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Strict, E> {
                Err(E::custom("non-integer token"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Strict, E> {
                if scalar(v) {
                    Ok(Strict(v.into()))
                } else {
                    Err(E::custom("noncharacter"))
                }
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Strict, E> {
                self.visit_str(&v)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = vec![];
                while let Some(Strict(x)) = a.next_element()? {
                    v.push(x)
                }
                Ok(Strict(v.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Strict, A::Error> {
                let mut v = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if !scalar(&k) || v.contains_key(&k) {
                        return Err(de::Error::custom("invalid or duplicate key"));
                    }
                    let Strict(x) = a.next_value()?;
                    v.insert(k, x);
                }
                Ok(Strict(v.into()))
            }
        }
        d.deserialize_any(V)
    }
}
/// Parses bytes as encoding/1 JSON, refusing what the service refuses.
pub fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<Strict>(bytes).map(|s| s.0)
}

/// The canonical encoding the service digests a command intent with: object
/// keys sorted by UTF-16 code units, no whitespace (JCS for encoding/1's
/// value domain, which has no floats).
pub fn canonical(value: &Value) -> Vec<u8> {
    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Object(map) => {
                out.push('{');
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort_by_cached_key(|k| k.encode_utf16().collect::<Vec<_>>());
                for (index, key) in keys.into_iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(key).unwrap_or_default());
                    out.push(':');
                    write(&map[key], out);
                }
                out.push('}');
            }
            Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other).unwrap_or_default()),
        }
    }
    let mut out = String::new();
    write(value, &mut out);
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strict_parse_refuses_what_the_service_refuses() {
        assert_eq!(parse(br#"{"a":[1,"x",null,true]}"#).unwrap()["a"][1], "x");
        assert!(parse(br#"{"a":1,"a":2}"#).is_err(), "a duplicate key");
        assert!(parse(br#"{"a":1.5}"#).is_err(), "a float");
        assert!(
            parse(br#"{"a":9007199254740992}"#).is_err(),
            "an unsafe integer"
        );
        assert!(
            parse("{\"a\":\"\u{fdd0}\"}".as_bytes()).is_err(),
            "a noncharacter"
        );
        // serde_json alone would take the first two.
        assert!(serde_json::from_slice::<Value>(br#"{"a":1,"a":2}"#).is_ok());
    }
}
