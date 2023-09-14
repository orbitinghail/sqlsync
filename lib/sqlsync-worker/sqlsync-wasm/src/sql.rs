use serde::{de::Visitor, Deserialize, Serialize};
use sqlsync::sqlite::{
    self,
    types::{ToSqlOutput, ValueRef},
    ToSql,
};
use wasm_bindgen::prelude::wasm_bindgen;

#[derive(Debug, Clone)]
pub enum SqlValue {
    /// The value is a `NULL` value.
    Null,
    /// The value is a signed integer.
    Integer(i64),
    /// The value is a floating point number.
    Real(f64),
    /// The value is a text string.
    Text(String),
    /// The value is a blob of data
    Blob(Vec<u8>),
}

#[wasm_bindgen(typescript_custom_section)]
const JS_SQL_VALUE_TYPESCRIPT: &'static str = r#"
export type SqlValue =
    | undefined
    | null
    | boolean
    | number
    | string
    | bigint
    | Uint8Array;
"#;

impl ToSql for SqlValue {
    fn to_sql(&self) -> sqlite::Result<ToSqlOutput<'_>> {
        match self {
            SqlValue::Null => Ok(ToSqlOutput::Borrowed(ValueRef::Null)),
            SqlValue::Integer(v) => Ok(ToSqlOutput::Borrowed(ValueRef::Integer(*v))),
            SqlValue::Real(v) => Ok(ToSqlOutput::Borrowed(ValueRef::Real(*v))),
            SqlValue::Text(v) => Ok(ToSqlOutput::Borrowed(ValueRef::Text(v.as_bytes()))),
            SqlValue::Blob(v) => Ok(ToSqlOutput::Borrowed(ValueRef::Blob(v.as_slice()))),
        }
    }
}

impl From<ValueRef<'_>> for SqlValue {
    fn from(value: ValueRef<'_>) -> Self {
        match value {
            ValueRef::Null => SqlValue::Null,
            ValueRef::Integer(v) => SqlValue::Integer(v),
            ValueRef::Real(v) => SqlValue::Real(v),
            r @ ValueRef::Text(_) => SqlValue::Text(r.as_str().unwrap().into()),
            ValueRef::Blob(v) => SqlValue::Blob(v.to_vec()),
        }
    }
}

impl Serialize for SqlValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            SqlValue::Null => serializer.serialize_none(),
            SqlValue::Integer(i) => serializer.serialize_i64(i),
            SqlValue::Real(f) => serializer.serialize_f64(f),
            SqlValue::Text(ref s) => serializer.serialize_str(s),
            SqlValue::Blob(ref b) => serializer.serialize_bytes(b),
        }
    }
}

impl<'de> Deserialize<'de> for SqlValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct SqlValueVisitor;

        impl<'de> Visitor<'de> for SqlValueVisitor {
            type Value = SqlValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("SqlValue")
            }

            fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Integer(if v { 1 } else { 0 }))
            }

            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Integer(v))
            }

            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Real(v))
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Text(v.to_string()))
            }

            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Blob(v.to_vec()))
            }

            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Blob(v))
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Null)
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(SqlValue::Null)
            }
        }

        deserializer.deserialize_any(SqlValueVisitor)
    }
}
