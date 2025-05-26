use std::time::Duration;

use tokio_stream::StreamExt;

use serde_json::{from_slice, from_str, to_string};

use crate::core::controller::OperationController;
use crate::core::operation::{OpState, OperationInfo};
use crate::core::serde::AnySerializable;
use crate::core::traits::Identity;

use crate::testkit::conditions::TestIntervalCondition;
use crate::testkit::operation::TestOperation;
use crate::testkit::params::TestRetry;
use crate::testkit::tracing::init_tracing;

#[cfg(test)]
mod tests_serde_core_types {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn test_serde_op_info() {
        let op_info = OperationInfo::from_str("Test Task", "12345", false, false, false);

        let serialized = to_string(&op_info).expect("Failed to serialize OperationInfo");
        let deserialized: OperationInfo =
            from_str(&serialized).expect("Failed to deserialize OperationInfo");

        assert_eq!(op_info.description(), deserialized.description());
        assert_eq!(op_info.id(), deserialized.id());
        assert_eq!(op_info.active(), deserialized.active());
    }

    #[test]
    fn test_serde_op_state() {
        let stages = vec![
            OpState::Active,
            OpState::Panicked("Unexpected Error".to_string()),
            OpState::Aborted,
            OpState::Ok(None),
            OpState::Ok(Some(Cow::Borrowed("Result"))),
            OpState::Failed(None),
            OpState::Failed(Some(Cow::Borrowed("Expected Error"))),
            OpState::Locked(None),
            OpState::Locked(Some(Cow::Borrowed("Locked Reason"))),
        ];

        for stage in stages {
            let serialized = to_string(&stage).expect("Failed to serialize OpState");

            let deserialized: OpState =
                from_str(&serialized).expect("Failed to deserialize OpState");

            assert_eq!(stage, deserialized);
        }
    }

    #[test]
    fn test_serde_any_serializable() {
        let value = TestRetry::new(3, 1000);
        let any_serializable = AnySerializable::new_register(value.clone());

        // Serialize
        let serialized = serde_json::to_vec(&any_serializable).unwrap();

        // Deserialize
        let deserialized: AnySerializable = from_slice(&serialized).unwrap();

        assert_eq!(any_serializable, deserialized);

        // Downcast to expected type
        assert_eq!(&value, deserialized.downcast_ref::<TestRetry>().unwrap());
    }
}

#[cfg(test)]
mod tests_default_controller {
    use super::*;
    use crate::core::traits::IntoSensor;
    use std::borrow::Cow;

    #[test]
    fn test_new() {
        let controller = OperationController::new("test_controller");
        assert!(controller.is_empty());
    }

    #[test]
    fn test_submit() {
        let op = TestOperation::ok("test operation", "this is test operation", None);
        let mut controller = OperationController::new("test_controller");
        controller.submit(op, None);
        assert_eq!(controller.count(), 1);
    }

    #[tokio::test]
    async fn test_activate_op_stream() {
        init_tracing();
        let op = TestOperation::ok("test operation", "this is test operation", None);
        let id = op.id();
        let mut controller = OperationController::new("test_controller");
        controller.submit(op, None);
        match controller.activate_stream::<()>(id, None) {
            Ok(mut watch_stream) => {
                let mut last_state: Option<OpState> = None;
                while let Some(state) = watch_stream.next().await {
                    last_state = Some(state);
                }
                assert_eq!(
                    last_state.expect("No state received"),
                    OpState::Ok(Some(Cow::Borrowed("Result"))),
                );
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    #[tokio::test]
    async fn test_activate_op_stream_params() {
        init_tracing();
        // New operation that fails to test retry params
        let operation = TestOperation::err("test operation", "this is test operation", None);
        let id = operation.id();
        let mut controller = OperationController::new("test_controller");

        // Submit operation and register parameters
        controller.submit_parameters::<TestRetry>(operation, None);

        let params = AnySerializable::new(TestRetry::new(3, 1000));
        match controller.activate_stream(id, Some(params)) {
            Ok(mut watch_stream) => {
                let mut last_state: Option<OpState> = None;
                while let Some(state) = watch_stream.next().await {
                    last_state = Some(state);
                }
                assert_eq!(
                    last_state.expect("No state received"),
                    OpState::Failed(Some(Cow::Borrowed("Expected Error"))),
                );
            }
            Err(e) => panic!("Failed to run task: {:?}", e),
        }
    }

    #[tokio::test]
    async fn test_set_sensor() {
        init_tracing();
        let op = TestOperation::ok("test operation", "test operation", None);
        let op_id = op.id();
        let sensor = TestIntervalCondition::new(1, None).into_sensor();
        let mut controller = OperationController::new("test_controller");
        controller.submit(op, None);
        // Not set
        assert!(controller.is_sensor_active(&op_id).is_err());
        let op_2 = TestOperation::ok("test operation 2", "test operation", None);
        let op_2_id = op_2.id();
        controller.submit(op_2, Some(sensor));
        assert!(!controller.is_sensor_active(&op_2_id).unwrap());
    }

    #[tokio::test]
    async fn test_sensor() {
        init_tracing();
        let op = TestOperation::ok("test operation", "this is test operation", None);
        let op_id = op.id();
        // one second interval for activation
        let sensor = TestIntervalCondition::new(1, None).into_sensor();
        let mut controller = OperationController::new("test_controller");
        controller.submit(op, Some(sensor));
        // Neither sensor nor operation should be active at this point
        assert!(!controller.is_sensor_active(&op_id).unwrap());
        assert!(!controller.is_active(&op_id).unwrap());
        // Active sensor
        controller.activate_sensor(&op_id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        // Sensor should be active
        assert!(controller.is_sensor_active(&op_id).unwrap());
        controller.deactivate_sensor(&op_id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;
        // Neither sensor nor operation should be active now
        assert!(!controller.is_sensor_active(&op_id).unwrap());
        assert!(!controller.is_active(&op_id).unwrap());
    }
}
