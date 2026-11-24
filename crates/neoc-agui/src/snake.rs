//! The crate's values with the Python SDK's attribute names: `thread_id` where the wire has `threadId`.
//!
//! [`Snake`] wraps any serializer and rewrites the names of struct fields, and only those. A struct field is part
//! of the protocol; a map key is somebody's data (the state, an activity's content, a tool's schema), and is
//! written as it came. That is why no type of this crate uses `#[serde(flatten)]`, which writes a struct as a map.

use serde::ser::{
    Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// `value`, written to `serializer` with `snake_case` struct fields.
///
/// # Errors
/// The serializer's own.
pub fn serialize<T: Serialize + ?Sized, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    value.serialize(Snake(serializer))
}

/// `value` as JSON with `snake_case` struct fields.
///
/// # Errors
/// The value cannot be JSON (a map whose keys are not strings).
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value, serde_json::Error> {
    serialize(value, serde_json::value::Serializer)
}

/// `threadId` as `thread_id`. The names are static, so each distinct one is converted and kept once.
fn snake(name: &'static str) -> &'static str {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock, PoisonError};
    static NAMES: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();

    if !name.bytes().any(|b| b.is_ascii_uppercase()) {
        return name;
    }
    let mut names = NAMES.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner);
    names.entry(name).or_insert_with(|| {
        let mut out = String::with_capacity(name.len() + 2);
        for c in name.chars() {
            if c.is_ascii_uppercase() {
                out.push('_');
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(c);
            }
        }
        // one small string for each field name of the crate's types, for the life of the process
        Box::leak(out.into_boxed_str())
    })
}

/// A serializer that writes struct fields in `snake_case` and everything else as `S` does.
pub struct Snake<S>(pub S);

/// A value inside one being written by a [`Snake`].
struct Inner<'a, T: ?Sized>(&'a T);

impl<T: Serialize + ?Sized> Serialize for Inner<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(Snake(serializer))
    }
}

macro_rules! forward {
    ($( $method:ident($ty:ty) )*) => {
        $( fn $method(self, v: $ty) -> Result<S::Ok, S::Error> { self.0.$method(v) } )*
    };
}

impl<S: Serializer> Serializer for Snake<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Snake<S::SerializeSeq>;
    type SerializeTuple = Snake<S::SerializeTuple>;
    type SerializeTupleStruct = Snake<S::SerializeTupleStruct>;
    type SerializeTupleVariant = Snake<S::SerializeTupleVariant>;
    type SerializeMap = Snake<S::SerializeMap>;
    type SerializeStruct = Snake<S::SerializeStruct>;
    type SerializeStructVariant = Snake<S::SerializeStructVariant>;

    forward! {
        serialize_bool(bool) serialize_i8(i8) serialize_i16(i16) serialize_i32(i32) serialize_i64(i64)
        serialize_i128(i128) serialize_u8(u8) serialize_u16(u16) serialize_u32(u32) serialize_u64(u64)
        serialize_u128(u128) serialize_f32(f32) serialize_f64(f64) serialize_char(char) serialize_str(&str)
        serialize_bytes(&[u8])
    }

    fn serialize_none(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_none()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<S::Ok, S::Error> {
        self.0.serialize_some(&Inner(value))
    }
    fn serialize_unit(self) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_struct(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_unit_variant(name, index, variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_struct(name, &Inner(value))
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<S::Ok, S::Error> {
        self.0.serialize_newtype_variant(name, index, variant, &Inner(value))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, S::Error> {
        self.0.serialize_seq(len).map(Snake)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, S::Error> {
        self.0.serialize_tuple(len).map(Snake)
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, S::Error> {
        self.0.serialize_tuple_struct(name, len).map(Snake)
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, S::Error> {
        self.0.serialize_tuple_variant(name, index, variant, len).map(Snake)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, S::Error> {
        self.0.serialize_map(len).map(Snake)
    }
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeStruct, S::Error> {
        self.0.serialize_struct(name, len).map(Snake)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, S::Error> {
        self.0.serialize_struct_variant(name, index, variant, len).map(Snake)
    }
    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

macro_rules! elements {
    ($( $trait:ident :: $method:ident )*) => { $(
        impl<S: $trait> $trait for Snake<S> {
            type Ok = S::Ok;
            type Error = S::Error;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
                self.0.$method(&Inner(value))
            }
            fn end(self) -> Result<S::Ok, S::Error> {
                self.0.end()
            }
        }
    )* };
}

elements! {
    SerializeSeq::serialize_element SerializeTuple::serialize_element SerializeTupleStruct::serialize_field
    SerializeTupleVariant::serialize_field
}

impl<S: SerializeMap> SerializeMap for Snake<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    // a key is data: written as it is
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), S::Error> {
        self.0.serialize_key(key)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), S::Error> {
        self.0.serialize_value(&Inner(value))
    }
    fn end(self) -> Result<S::Ok, S::Error> {
        self.0.end()
    }
}

macro_rules! fields {
    ($( $trait:ident )*) => { $(
        impl<S: $trait> $trait for Snake<S> {
            type Ok = S::Ok;
            type Error = S::Error;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), S::Error> {
                self.0.serialize_field(snake(key), &Inner(value))
            }
            fn skip_field(&mut self, key: &'static str) -> Result<(), S::Error> {
                self.0.skip_field(snake(key))
            }
            fn end(self) -> Result<S::Ok, S::Error> {
                self.0.end()
            }
        }
    )* };
}

fields! { SerializeStruct SerializeStructVariant }

#[cfg(test)]
mod tests {
    use crate::events::Event;
    use serde_json::json;

    #[test]
    fn struct_fields_are_renamed_and_the_data_inside_them_is_not() {
        let event = Event::from_json(
            &json!({
                "type": "MESSAGES_SNAPSHOT",
                "rawEvent": { "keepThis": { "andThis": 1 } },
                "messages": [
                    { "role": "assistant", "id": "1", "toolCalls": [
                        { "id": "c", "type": "function", "function": { "name": "f", "arguments": "{\"camelArg\":1}" } } ] },
                    { "role": "activity", "id": "2", "activityType": "a2ui-surface", "content": { "surfaceId": "s" } },
                    { "role": "tool", "id": "3", "toolCallId": "c", "content": "ok" },
                ],
            })
            .to_string(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            super::to_value(&event).unwrap(),
            json!({
                "type": "MESSAGES_SNAPSHOT",
                "raw_event": { "keepThis": { "andThis": 1 } },
                "messages": [
                    { "role": "assistant", "id": "1", "tool_calls": [
                        { "id": "c", "type": "function", "function": { "name": "f", "arguments": "{\"camelArg\":1}" } } ] },
                    { "role": "activity", "id": "2", "activity_type": "a2ui-surface", "content": { "surfaceId": "s" } },
                    { "role": "tool", "id": "3", "tool_call_id": "c", "content": "ok" },
                ],
            })
        );
    }
}
