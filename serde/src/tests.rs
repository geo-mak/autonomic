use serde_json::from_slice;

use crate::dynamic::AnySerializable;
use crate::testkit::schema::TestCustomSchema;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_any_serializable() {
        let schema = TestCustomSchema::new(3, 1000);
        let any_serializable = AnySerializable::new_register(schema.clone());

        // Serialize
        let serialized = serde_json::to_vec(&any_serializable).unwrap();

        // Deserialize
        let deserialized: AnySerializable = from_slice(&serialized).unwrap();

        assert_eq!(any_serializable, deserialized);

        // Downcast to expected type
        assert_eq!(
            &schema,
            deserialized.downcast_ref::<TestCustomSchema>().unwrap()
        );
    }
}
