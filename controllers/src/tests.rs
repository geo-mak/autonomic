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
        let ctrl_info = ControllerInfo::from_str("Test Task", "12345", 0, false);

        let serialized = to_string(&ctrl_info).expect("Failed to serialize OperationInfo");
        let deserialized: ControllerInfo =
            from_str(&serialized).expect("Failed to deserialize OperationInfo");

        assert_eq!(ctrl_info.description(), deserialized.description());
        assert_eq!(ctrl_info.id(), deserialized.id());
        assert_eq!(ctrl_info.performing(), deserialized.performing());
    }

    #[test]
    fn test_serde_op_state() {
        let stages = vec![
            OpState::Started,
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
}

#[cfg(test)]
mod tests_ctrl_manager {
    use crate::controller::Controller;

    use super::*;
    use std::borrow::Cow;

    static CTRL_ID: &str = "test_ctrl";

    #[test]
    fn test_submit() {
        let mut manager = ControllerManager::new();
        let controller = TestController::ok(CTRL_ID, None, 0);
        manager.submit(controller);
        assert_eq!(manager.count(), 1);
    }

    #[tokio::test]
    async fn test_preform() {
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
                assert_eq!(
                    last_state.expect("No state received"),
                    OpState::Ok(Some(Cow::Borrowed("Result"))),
                );
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    #[tokio::test]
    async fn test_sensor() {
        init_tracing();
        let manager = ControllerManager::new().into_static();

        let controller = TestController::ok(CTRL_ID, None, 1);
        let id = controller.id();

        manager.submit(controller);

        assert!(!manager.sensing(&id).unwrap());
        assert!(!manager.is_performing(&id).unwrap());

        manager.start_sensor(&id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Sensing should be active
        assert!(manager.sensing(&id).unwrap());
        manager.stop_sensor(&id).unwrap();
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Neither sensor nor operation should be active now.
        assert!(!manager.sensing(&id).unwrap());
        assert!(!manager.is_performing(&id).unwrap());
    }
}
