use alloc::borrow::{Cow, ToOwned as _};
use alloc::vec::Vec;
use schemars::_private::serde_json::{Map, Value, json};
use schemars::Schema;
use schemars::SchemaGenerator;

use crate::{BackwardsCompat, Migrated};

/// Version tag literal value used when building a JSON Schema for a schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagSchemaValue {
    /// Integer version tag (e.g. `1`).
    Int(u64),
    /// String version tag (e.g. `"1.0"`).
    Str(&'static str),
}

impl TagSchemaValue {
    fn into_json_schema(self) -> Value {
        match self {
            Self::Int(n) => json!({
                "type": "integer",
                "const": n,
            }),
            Self::Str(s) => json!({
                "type": "string",
                "const": s,
            }),
        }
    }
}

/// Decorates a version subschema with an internally-tagged version property and requirement.
#[must_use]
pub fn schema_internally_tagged(mut subschema: Schema, tag_field: &str, tag: TagSchemaValue) -> Schema {
    let tag_prop = tag.into_json_schema();

    let is_compound_or_ref = subschema.get("$ref").is_some()
        || subschema.get("oneOf").is_some()
        || subschema.get("anyOf").is_some()
        || subschema.get("allOf").is_some()
        || subschema.as_bool().is_some();

    if is_compound_or_ref {
        return schemars::json_schema!({
            "allOf": [
                subschema,
                {
                    "type": "object",
                    "properties": {
                        tag_field: tag_prop,
                    },
                    "required": [tag_field],
                }
            ]
        });
    }

    let obj = subschema.ensure_object();
    if obj.get("type").and_then(Value::as_str) == Some("null") || !obj.contains_key("type") {
        obj.insert("type".to_owned(), Value::String("object".to_owned()));
    }

    if let Some(properties) = obj
        .entry("properties")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
    {
        properties.insert(tag_field.to_owned(), tag_prop);
    }

    if let Some(required) = obj
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
    {
        let tag_val = Value::String(tag_field.to_owned());
        if !required.contains(&tag_val) {
            required.insert(0, tag_val);
        }
    }

    subschema
}

/// Wraps a version subschema in an adjacently-tagged `{ tag_field: <const>, content_field: <subschema> }` object.
#[must_use]
pub fn schema_adjacently_tagged(
    subschema: Schema,
    tag_field: &str,
    content_field: &str,
    tag: TagSchemaValue,
) -> Schema {
    let mut properties = Map::new();
    properties.insert(tag_field.to_owned(), tag.into_json_schema());
    properties.insert(content_field.to_owned(), subschema.into());

    let mut root = Map::new();
    root.insert("type".to_owned(), Value::String("object".to_owned()));
    root.insert("properties".to_owned(), Value::Object(properties));
    root.insert(
        "required".to_owned(),
        Value::Array(alloc::vec![
            Value::String(tag_field.to_owned()),
            Value::String(content_field.to_owned()),
        ]),
    );
    Schema::from(root)
}

/// Combines multiple version subschemas into a single schema (`oneOf` when > 1).
#[must_use]
pub fn schema_one_of(schemas: Vec<Schema>) -> Schema {
    let mut iter = schemas.into_iter();
    let Some(first) = iter.next() else {
        return Schema::from(false);
    };
    let Some(second) = iter.next() else {
        return first;
    };
    let mut all = Vec::with_capacity(2 + iter.len());
    all.push(first);
    all.push(second);
    all.extend(iter);
    schemars::json_schema!({
        "oneOf": all,
    })
}

impl<T: BackwardsCompat + schemars::JsonSchema> schemars::JsonSchema for Migrated<T> {
    fn inline_schema() -> bool {
        T::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        T::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        T::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        T::json_schema(generator)
    }
}
