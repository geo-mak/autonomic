use std::marker::PhantomData;
use std::sync::Arc;

use tokio::sync::broadcast;

use tracing::{Event, Subscriber};

use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::events::traits::EventRecorder;

/// Reference to the events channel for creating receivers.
///
/// > **Notes**:
/// > - The channel has shared buffer for sent events, that are not yet received by subscribers.
/// > - Each subscriber receives its own copy of the event.
/// > - If subscriber is slow to receive sent events, it can still receive missed events as long the channel's buffer is not full.
/// > - When the buffer becomes full, the oldest event is dropped to make space for new events.
/// > - Subscriber that has not yet received the dropped event will receive the error `RecvError::Lagged`.
/// > - The lagged subscribers will receive again from the oldest event remained in channel's buffer.
/// > - When the channel is dropped, subscribers will receive the error `RecvError::Closed`.
pub type EventChannel<T> = Arc<broadcast::Sender<T>>;

/// Publishing layer for tracing subscribers.
/// Publisher intercepts events and publishes them to a broadcast channel.
///
/// # Generic Parameters
/// - `S`: tracing subscriber that accepts `Layer<S>` types.
/// - `R`: The recorder type that filters and records events.
pub struct EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder,
{
    _subscriber: PhantomData<S>,
    channel: EventChannel<R::Schema>,
}

impl<S, R> EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder,
{
    /// Creates new `EventPublisher` instance.
    ///
    /// # Parameters
    /// -`buffer`: The size of the channel's buffer to retain unreceived events.
    ///
    /// > **Note**: Buffer size must be greater than `16`. If value is less than `16`, it will be set to `16`.
    pub fn new(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel::<R::Schema>(buffer.max(16));
        Self {
            _subscriber: PhantomData,
            channel: Arc::new(tx),
        }
    }

    /// An atomic reference to the channel.
    ///
    /// Subscribers can be created from the channel using `subscribe()` method.
    ///
    /// > Note: Recording and publishing events will be **suspended**, when there are **no active subscribers** to receive events.
    pub fn channel(&self) -> EventChannel<R::Schema> {
        self.channel.clone()
    }
}

impl<S, R> Layer<S> for EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder + 'static,
{
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        if self.channel.receiver_count() != 0 {
            if let Some(schema) = R::record(event) {
                let _ = self.channel.send(schema);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;

    use tracing::subscriber;
    use tracing_core::Level;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::Registry;

    use crate::events::record::{DefaultEvent, PublisherRecorder};
    use crate::{trace_error, trace_info};

    #[tokio::test]
    async fn test_publisher_publish_stop_on_error() {
        // -------------------- Shared Setup ----------------------

        // New publisher
        let publisher = EventPublisher::<Registry, PublisherRecorder>::new(16);

        // Create a reference to channel before moving publisher
        let channel: EventChannel<DefaultEvent> = publisher.channel();

        // Initialize the tracing subscriber with the publisher as layer
        let tracing_subscriber = Registry::default().with(publisher); // Publisher moved

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(tracing_subscriber);

        // Create subscriber to publisher's channel
        let mut events_receiver = channel.subscribe();

        // -------------------- Test Published Events ----------------------

        // Propagate two published events
        trace_info!(
            source = "published event 1",
            message = "info message",
            published
        );

        trace_error!(
            source = "published event 2",
            message = "error message",
            published
        );

        // We must have two unreceived events in the channel
        assert_eq!(channel.len(), 2);

        // -------------------- Test Expected Events ----------------------

        // Expected INFO event
        let event_1 = DefaultEvent::new(
            &Level::INFO,
            "published event 1".to_string(),
            "info message".to_string(),
            "target".to_string(),
            Utc::now(),
        );

        // Expected ERROR event
        let event_2 = DefaultEvent::new(
            &Level::ERROR,
            "published event 2".to_string(),
            "error message".to_string(),
            "target".to_string(),
            Utc::now(),
        );

        // Receive events
        let received_event1 = events_receiver.recv().await.unwrap();
        let received_event2 = events_receiver.recv().await.unwrap();

        // Assert subscriber received the expected events
        assert_eq!(received_event1.source(), event_1.source());
        assert_eq!(received_event1.message(), event_1.message());

        assert_eq!(received_event2.source(), event_2.source());
        assert_eq!(received_event2.message(), event_2.message());

        // -------------------- Test Unpublished Events ----------------------

        // Propagate unpublished event
        trace_info!(source = "unpublished event", message = "info message");

        // Sine the event is unpublished, it must have been ignored by the publisher
        assert!(events_receiver.is_empty());

        // -------------------- Test Suspending Publishing ----------------------

        // Now we simulate the scenario where all receivers are dropped
        drop(events_receiver);

        assert_eq!(channel.receiver_count(), 0);

        // Propagate event
        trace_info!(
            source = "published event",
            message = "info message",
            published
        );

        // No event should have been recorded and published
        assert!(channel.is_empty())
    }
}
