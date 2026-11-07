//! Python addons/log2reqs/yaml.py の設定契約。
use serde::Deserialize;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "utf8", deserialize_with = "null_utf8")]
    pub encoding: String,
}
fn utf8() -> String {
    "utf8".into()
}
crate::models::defaults::null_default_with!(null_utf8, String, utf8());

impl Config {
    pub fn read(&self, body: &[u8]) -> crate::compat::Result<Vec<crate::models::Request>> {
        let text = crate::compat::encoding::decode_input(body, &self.encoding)?;
        let mut value =
            serde_yaml_ng::from_str::<FiniteJson>(text.strip_prefix('\u{feff}').unwrap_or(&text))
                .map_err(|e| {
                    crate::compat::Error::new(
                        if e.to_string().contains("non-finite") {
                            "ValueError"
                        } else {
                            "ParserError"
                        },
                        e.to_string(),
                    )
                })?
                .0;
        crate::config::expand_yaml_merges(&mut value, "requests")?;
        super::normalize_keys(&mut value);
        super::requests(value)
    }
}

// serde_json::Valueのvisit_f64は非有限値をnullへ変換する。その前に明示エラーにする。
struct FiniteJson(serde_json::Value);
impl<'de> serde::Deserialize<'de> for FiniteJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};
        use serde_json::{Map, Number, Value};
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = FiniteJson;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a finite JSON value")
            }
            fn visit_unit<E: de::Error>(self) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<FiniteJson, E> {
                self.visit_unit()
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::Number(v.into())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<FiniteJson, E> {
                Number::from_f64(v)
                    .map(|v| FiniteJson(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite number cannot be represented in JSON"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<FiniteJson, E> {
                Ok(FiniteJson(Value::String(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<FiniteJson, A::Error> {
                let mut values = Vec::new();
                while let Some(FiniteJson(v)) = access.next_element()? {
                    values.push(v);
                }
                Ok(FiniteJson(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<FiniteJson, A::Error> {
                let mut values = Map::new();
                while let Some((k, FiniteJson(v))) = access.next_entry::<String, FiniteJson>()? {
                    values.insert(k, v);
                }
                Ok(FiniteJson(Value::Object(values)))
            }
            fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<FiniteJson, A::Error> {
                let (_, value) = access.variant::<String>()?;
                value.newtype_variant()
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}
