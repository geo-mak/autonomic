use std::marker::PhantomData;

use tokio::sync::broadcast;

use tracing::{Event, Subscriber};

use tracing_subscriber::Layer;
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Context;

use crate::layer::filter::CallSiteFilter;
use crate::traits::EventRecorder;

/// Reference to the events channel for creating receivers.
///
/// > **Notes**:
/// > - The channel has shared buffer for sent events, that are not yet received by subscribers.
/// > - Each subscriber receives its own copy of the event.
/// > - If subscriber is slow to receive sent events, it can still receive missed events
/// > as long the channel's buffer is not full.
/// > - When the buffer becomes full, the oldest event is dropped to make space for new events.
/// > - Subscriber that has not yet received the dropped event will receive the error `RecvError::Lagged`.
/// > - The lagged subscribers will receive again from the oldest event remained in channel's buffer.
/// > - When the channel is dropped, subscribers will receive the error `RecvError::Closed`.
pub type EventChannel<T> = broadcast::Sender<T>;
pub type PublisherLayer<S, R, D> = Filtered<EventPublisher<S, R>, CallSiteFilter<S, D>, S>;

/// Publisher layer for tracing subscribers.
pub struct EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder,
{
    _subscriber: PhantomData<S>,
    channel: EventChannel<R::Record>,
}

impl<S, R> EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder<Record: Clone> + 'static,
{
    /// Creates new `EventPublisher` instance.
    ///
    /// # Parameters
    /// -`buffer`: The size of the channel's buffer to retain unreceived events.
    ///
    /// **Note**: Buffer size must be greater than `16`.
    /// If value is less than `16`, it will be set to `16`.
    ///
    /// # Example
    /// ```rust,no_run
    /// use tracing::subscriber;
    /// use tracing_core::Level;
    /// use tracing_subscriber::Registry;
    /// use tracing_subscriber::layer::SubscriberExt;
    ///
    /// use autonomic_events::layer::publisher::{EventChannel, EventPublisher};
    /// use autonomic_events::record::{DefaultEvent, DefaultRecorder};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     // A new publisher with the default recorder and directive
    ///     let (channel, publisher) = EventPublisher::<Registry, DefaultRecorder>::new(16);
    ///
    ///     // Initialize the tracing subscriber with publisher as layer
    ///     let tracing_subscriber = Registry::default().with(publisher);
    ///
    ///     // Make subscriber a valid default for the entire test
    ///     let _guard = subscriber::set_default(tracing_subscriber);
    ///
    ///     // Create subscriber to publisher's channel
    ///     let mut events_receiver = channel.subscribe();
    /// }
    /// ```
    #[must_use]
    pub fn new(buffer: usize) -> (EventChannel<R::Record>, PublisherLayer<S, R, R::Directive>) {
        let (tx, _) = broadcast::channel::<R::Record>(buffer.max(16));
        let instance = Self {
            _subscriber: PhantomData,
            channel: tx,
        };
        (
            instance.channel.clone(),
            Filtered::new(instance, CallSiteFilter::new()),
        )
    }
}

impl<S, R> Layer<S> for EventPublisher<S, R>
where
    S: Subscriber,
    R: EventRecorder<Record: Clone> + 'static,
{
    // > Note: Disabling event per call-site for this layer is done by the filter.
    // > It can't be done in layer using method `register_callsite`, because it will disable it
    // globally for all layers, and this is the only reason why layer is fused with the filter.

    // This method is called before `on_event` and doesn't disable recording permanently
    #[inline]
    fn event_enabled(&self, _event: &Event<'_>, _ctx: Context<'_, S>) -> bool {
        // Recording is suspended when there are no active subscribers
        self.channel.receiver_count() != 0
    }

    #[inline]
    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        let _ = self.channel.send(R::record(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tracing::subscriber;
    use tracing_core::Level;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::{record::DefaultRecorder, trace_error, trace_info};

    #[tokio::test]
    async fn test_publisher_publish_stop_on_error() {
        // -------------------- Shared Setup ----------------------

        // A new publisher with the default recorder and directive
        let (channel, publisher) = EventPublisher::<Registry, DefaultRecorder>::new(16);

        // Initialize the tracing subscriber with publisher as layer
        let tracing_subscriber = Registry::default().with(publisher);

        // Make subscriber a valid default for the entire test
        let _guard = subscriber::set_default(tracing_subscriber);

        // Create subscriber to publisher's channel
        let mut events_receiver = channel.subscribe();

        // -------------------- Test Published Events ----------------------

        // Propagate two published events
        trace_info!(message = "info message");

        trace_error!(message = "error message");

        // We must have two unreceived events in the channel
        assert_eq!(channel.len(), 2);

        // -------------------- Test Expected Events ----------------------

        // Receive events
        let received_event1 = events_receiver.recv().await.unwrap();
        let received_event2 = events_receiver.recv().await.unwrap();

        assert_eq!(received_event1.tracing_level(), Level::INFO);
        assert_eq!(received_event1.message(), "info message");

        assert_eq!(received_event2.tracing_level(), Level::ERROR);
        assert_eq!(received_event2.message(), "error message");

        // -------------------- Test Suspending Publishing ----------------------

        // Now we simulate the scenario where all receivers are dropped
        drop(events_receiver);

        assert_eq!(channel.receiver_count(), 0);

        // Propagate event
        trace_info!(message = "info message");

        // No event should have been recorded and published
        assert!(channel.is_empty())
    }
}
