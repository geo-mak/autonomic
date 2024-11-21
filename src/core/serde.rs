use std::any::{type_name, Any};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display};
use std::sync::RwLock;

use serde::de::{Error, Visitor};
use serde::ser::SerializeStruct;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use lazy_static::lazy_static;

use crate::core::operation::OperationParameters;

/// Serializable trait as marker for serializable types.
/// Types implementing this trait can be serialized, deserialized, and cloned.
pub trait GenericSerializable: Any + Send + Sync + Debug {
    fn serialize(&self) -> Result<Vec<u8>, String>;
    fn as_any(&self) -> &dyn Any;
    fn clone_boxed(&self) -> Box<dyn GenericSerializable>;
}

pub trait IntoAnySerializable: GenericSerializable + for<'de> Deserialize<'de> {
    fn into_any_serializable(self) -> AnySerializable;
}

type DeserializeFn = fn(&[u8]) -> Box<dyn GenericSerializable>;

// Table to store the deserialization function for each registered type.
lazy_static! {
    static ref DESERIALIZE_TABLE: RwLock<HashMap<&'static str, DeserializeFn>> =
        RwLock::new(HashMap::new());
}

pub struct DeserializeRegistry;

impl DeserializeRegistry {
    /// Registers a type to be deserializable.
    /// This function adds the type's deserialization function to the `DESERIALIZE_TABLE`.
    pub fn register<T>() -> &'static str
    where
        T: GenericSerializable + for<'de> Deserialize<'de>,
    {
        let type_name = type_name::<T>();
        let deserialize_fn: DeserializeFn =
            |s| Box::new(serde_json::from_slice::<T>(s).expect("Failed to deserialize"));

        // Check if type is already registered to avoid locking
        {
            let read_lock = DESERIALIZE_TABLE
                .read()
                .expect("Failed to acquire read lock");
            if read_lock.contains_key(&type_name) {
                return type_name;
            }
        }

        // Lock access and register type
        {
            let mut write_lock = DESERIALIZE_TABLE
                .write()
                .expect("Failed to acquire write lock");
            write_lock.insert(type_name, deserialize_fn);
        }

        // Return type name
        type_name
    }

    #[inline]
    pub fn deserialize(
        name: &str,
        data: &[u8],
    ) -> Result<Box<dyn GenericSerializable>, &'static str> {
        let read_lock = DESERIALIZE_TABLE
            .read()
            .expect("Failed to acquire read lock");
        match read_lock.get(name) {
            Some(f) => Ok(f(data)),
            None => Err("Unregistered type cannot be deserialized"),
        }
    }
}

/// Unique safe pointer with metadata about the heap-allocated data it owns.
/// Designed as a wrapper around any serializable type that can be serialized and deserialized.
///
/// > **Important Notes**:
/// > - Using this type is relatively an expensive operation, because it might need to register concrete type in global registry at runtime.
/// > - Always consider using a concrete type where possible.
/// > - Base size: `40 bytes` on `64 bit` systems.
pub struct AnySerializable {
    type_name: Cow<'static, str>,
    data: Box<dyn GenericSerializable>,
}

impl AnySerializable {
    /// Create a new `AnySerializable` from a serializable type.
    ///
    /// > **Note**: This method doesn't register the type.
    /// > If it needs to be deserialized in this process, use `new_register` method instead.
    pub fn new<T>(value: T) -> Self
    where
        T: GenericSerializable + for<'de> Deserialize<'de>,
    {
        AnySerializable {
            type_name: Cow::Borrowed(type_name::<T>()),
            data: Box::new(value),
        }
    }

    /// Create a new `AnySerializable` from a serializable type and registers it in the local registry.
    ///
    /// > **Note**: This method register the type in a global in-process registry.
    /// > If it doesn't need to be deserialized in this process, use `new` method instead.
    pub fn new_register<T>(value: T) -> Self
    where
        T: GenericSerializable + for<'de> Deserialize<'de>,
    {
        AnySerializable {
            type_name: Cow::Borrowed(DeserializeRegistry::register::<T>()),
            data: Box::new(value),
        }
    }

    /// Downcast the inner type to an immutable reference of the specified type.
    #[inline]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.data.as_any().downcast_ref::<T>()
    }
}

impl Serialize for AnySerializable {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let serialized_data = self.data.serialize().map_err(serde::ser::Error::custom)?;
        let mut state = serializer.serialize_struct("AnySerializable", 2)?;
        state.serialize_field("type_name", &self.type_name)?;
        state.serialize_field("data", &serialized_data)?;
        state.end()
    }
}

impl Clone for AnySerializable {
    fn clone(&self) -> Self {
        AnySerializable {
            type_name: self.type_name.clone(),
            data: self.data.clone_boxed(),
        }
    }
}

impl PartialEq for AnySerializable {
    fn eq(&self, other: &Self) -> bool {
        if self.type_name != other.type_name {
            return false;
        }
        self.data.serialize() == other.data.serialize()
    }
}

impl Debug for AnySerializable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "AnySerializable {{ type_name: {}, data: {:?} }}",
            self.type_name, self.data
        )
    }
}

impl Display for AnySerializable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.data)
    }
}

/// Visitor for deserializing AnySerializable.
struct AnySerializableVisitor;

impl<'de> Visitor<'de> for AnySerializableVisitor {
    type Value = AnySerializable;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a serializable value")
    }

    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: de::MapAccess<'de>,
    {
        let mut type_name: Option<Cow<'static, str>> = None;
        let mut data: Option<Vec<u8>> = None;

        while let Some(key) = map.next_key()? {
            match key {
                "type_name" => {
                    if type_name.is_some() {
                        return Err(Error::duplicate_field("type_name"));
                    }
                    type_name = Some(map.next_value()?);
                }
                "data" => {
                    if data.is_some() {
                        return Err(Error::duplicate_field("data"));
                    }
                    data = Some(map.next_value()?);
                }
                _ => {
                    return Err(Error::unknown_field(key, &["type_name", "data"]));
                }
            }
        }

        let type_name: Cow<'static, str> =
            type_name.ok_or_else(|| Error::missing_field("type_name"))?;

        let data: Vec<u8> = data.ok_or_else(|| Error::missing_field("data"))?;

        match DeserializeRegistry::deserialize(&type_name, &data) {
            Ok(data) => Ok(AnySerializable { type_name, data }),
            Err(e) => Err(Error::custom(e)),
        }
    }
}

impl<'de> Deserialize<'de> for AnySerializable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_struct(
            "AnySerializable",
            &["type_name", "data"],
            AnySerializableVisitor,
        )
    }
}

impl OperationParameters for AnySerializable {
    #[inline]
    fn as_parameters(&self) -> &dyn Any {
        self.data.as_any()
    }
}
