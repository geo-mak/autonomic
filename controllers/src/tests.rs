use std::time::Duration;

use tokio_stream::StreamExt;

use serde_json::{from_str, to_string};

use autonomic_events::testkit::global::init_tracing;

use crate::controller::{ControllerInfo, OpState};
use crate::manager::ControllerManager;
use crate::testkit::controller::TestController;

#[cfg(test)]
mod tests_serde {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn test_serde_ctrl_info() {
        let ctrl_info = ControllerInfo::from_str("Test Task", "12345", 0);

        let serialized = to_string(&ctrl_info).expect("Failed to serialize OperationInfo");
        let deserialized: ControllerInfo =
            from_str(&serialized).expect("Failed to deserialize OperationInfo");

        assert_eq!(ctrl_info.description(), deserialized.description());
        assert_eq!(ctrl_info.id(), deserialized.id());
        assert_eq!(ctrl_info.state(), deserialized.state());
    }

    #[test]
    fn test_serde_op_state() {
        let stages = vec![
            OpState::Started,
            OpState::Panicked("Unexpected Error".to_string()),
            OpState::Aborted,
            OpState::Ok,
            OpState::Failed(None),
            OpState::Failed(Some(Cow::Borrowed("Expected Error"))),
        ];

        for stage in stages {
            let serialized = to_string(&stage).expect("Failed to serialize OpState");

            let deserialized: OpState =
                from_str(&serialized).expect("Failed to deserialize OpState");

            assert_eq!(stage, deserialized);
        }
    }
}

#[cfg(test)]
mod tests_ctrl_manager {
    use crate::{controller::Controller, errors::ControllerError};

    use super::*;

    static CTRL_ID: &str = "test_ctrl";

    #[test]
    fn test_submit() {
        let mut manager = ControllerManager::new();
        let controller = TestController::ok(CTRL_ID, None, 0);
        manager.submit(controller);
        assert_eq!(manager.count(), 1);
    }

    #[test]
    #[should_panic(expected = "Controller with ID=test_ctrl already submitted")]
    fn test_submit_existing_controller() {
        let mut manager = ControllerManager::new();

        let controller1 = TestController::ok(CTRL_ID, None, 0);
        let controller2 = TestController::ok(CTRL_ID, None, 0);

        manager.submit(controller1);

        // This should panic
        manager.submit(controller2);
    }

    #[test]
    fn test_lock_unlock() {
        let mut manager = ControllerManager::new();
        let controller = TestController::ok(CTRL_ID, None, 0);

        manager.submit(controller);

        assert!(!manager.is_locked(CTRL_ID).unwrap());

        manager.lock(CTRL_ID).unwrap();
        assert!(manager.is_locked(CTRL_ID).unwrap());

        manager.unlock(CTRL_ID).unwrap();
        assert!(!manager.is_locked(CTRL_ID).unwrap());
    }

    #[tokio::test]
    async fn test_preform_ok() {
        init_tracing();
        let manager = ControllerManager::new().into_static();
        let controller = TestController::ok(CTRL_ID, None, 0);
        let ctrl_id = controller.id();

        manager.submit(controller);
        match manager.perform(ctrl_id) {
            Ok(mut watch_stream) => {
                let mut last_state: Option<OpState> = None;
                while let Some(state) = watch_stream.next().await {
                    last_state = Some(state);
                }
                assert_eq!(last_state.expect("No state received"), OpState::Ok);
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    #[tokio::test]
    async fn test_perform_panic() {
        init_tracing();
        let manager = ControllerManager::new().into_static();
        let controller = TestController::panic(CTRL_ID, None, 0);
        let ctrl_id = controller.id();

        manager.submit(controller);
        match manager.perform(ctrl_id) {
            Ok(mut watch_stream) => {
                let mut last_state: Option<OpState> = None;
                while let Some(state) = watch_stream.next().await {
                    last_state = Some(state);
                }
                assert_eq!(
                    last_state.expect("No state received"),
                    OpState::Panicked("Unexpected Error".to_string()),
                );

                assert!(manager.is_locked(ctrl_id).unwrap());
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    #[tokio::test]
    async fn test_abort_operation() {
        init_tracing();
        let manager = ControllerManager::new().into_static();
        let controller = TestController::ok(CTRL_ID, Some(Duration::from_millis(100)), 0);
        let ctrl_id = controller.id();

        manager.submit(controller);

        match manager.perform(ctrl_id) {
            Ok(mut watch_stream) => {
                tokio::time::sleep(Duration::from_millis(10)).await;

                assert!(manager.is_performing(ctrl_id).unwrap());

                manager.abort(ctrl_id).unwrap();

                let mut last_state: Option<OpState> = None;
                while let Some(state) = watch_stream.next().await {
                    last_state = Some(state);
                }

                assert_eq!(last_state.expect("No state received"), OpState::Aborted);
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    #[test]
    fn test_abort_non_performing_controller() {
        let mut manager = ControllerManager::new();

        let controller = TestController::ok(CTRL_ID, None, 0);

        manager.submit(controller);

        assert!(manager.abort(CTRL_ID).is_ok());
    }

    #[tokio::test]
    async fn test_start_sensor() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller = TestController::ok(CTRL_ID, None, 1);
        let id = controller.id();

        manager.submit(controller);

        assert!(!manager.sensing(&id).unwrap());

        manager.start_sensor(&id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Sensing should be active
        assert!(manager.sensing(&id).unwrap());
        manager.stop_sensor(&id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Neither sensor nor operation should be active now.
        assert!(!manager.sensing(&id).unwrap());

        manager.lock(&id).unwrap();
        let result = manager.start_sensor(&id);

        assert_eq!(result.unwrap_err(), ControllerError::Locked);
    }

    #[tokio::test]
    async fn test_start_sensor_panicked_op() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller = TestController::panic(CTRL_ID, None, 1);
        let id = controller.id();

        manager.submit(controller);

        match manager.perform(&id) {
            Ok(mut watch_stream) => while let Some(_state) = watch_stream.next().await {},
            Err(e) => panic!("{:?}", e),
        }

        assert!(manager.is_locked(&id).unwrap());

        let result = manager.start_sensor(&id);

        assert_eq!(result.unwrap_err(), ControllerError::Locked);

        assert!(!manager.sensing(&id).unwrap());
    }

    #[tokio::test]
    async fn test_active_sensor_with_panicked_op() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller = TestController::panic(CTRL_ID, None, 0);
        let id = controller.id();

        manager.submit(controller);

        let result = manager.start_sensor(&id);
        assert!(result.is_ok());

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!manager.sensing(id).unwrap());
        assert!(manager.is_locked(id).unwrap());
    }
}
