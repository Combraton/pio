//! Independent implementation of the pinned encoding/1 value domain and JCS subset.
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
pub fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<Strict>(bytes).map(|s| s.0)
}
pub fn canonical(value: &Value) -> Vec<u8> {
    fn write(v: &Value, s: &mut String) {
        match v {
            Value::Object(m) => {
                s.push('{');
                let mut keys: Vec<_> = m.keys().collect();
                keys.sort_by_cached_key(|k| k.encode_utf16().collect::<Vec<_>>());
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        s.push(',')
                    }
                    s.push_str(&serde_json::to_string(k).unwrap());
                    s.push(':');
                    write(&m[k], s)
                }
                s.push('}')
            }
            Value::Array(a) => {
                s.push('[');
                for (i, v) in a.iter().enumerate() {
                    if i > 0 {
                        s.push(',')
                    }
                    write(v, s)
                }
                s.push(']')
            }
            _ => s.push_str(&serde_json::to_string(v).unwrap()),
        }
    }
    let mut s = String::new();
    write(value, &mut s);
    s.into_bytes()
}
