use std::marker::PhantomData;
use std::sync::Arc;

use tokio::sync::broadcast;

use tracing::{Event, Subscriber};

use tracing_core::Metadata;

use tracing_subscriber::Layer;
use tracing_subscriber::filter::Filtered;
use tracing_subscriber::layer::Context;

use crate::layer::filter::CallSiteFilter;
use crate::record::DefaultRecorder;
use crate::traits::{EventRecorder, RecorderDirective};

const DEFAULT_EVENT_PUBLISHED: &str = "DEP";

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
pub type EventChannel<T> = Arc<broadcast::Sender<T>>;

/// Publisher directive enables recording default events marked as published.
pub struct PublisherDirective;

impl RecorderDirective for PublisherDirective {
    #[inline(always)]
    fn enabled(meta: &Metadata<'_>) -> bool {
        meta.name() == DEFAULT_EVENT_PUBLISHED
    }
}

/// Publisher layer for tracing subscribers.
/// This layer does not do any filtering.
pub struct PublisherLayer<S, R>
where
    S: Subscriber,
    R: EventRecorder,
{
    _subscriber: PhantomData<S>,
    channel: EventChannel<R::Record>,
}

impl<S, R> PublisherLayer<S, R>
where
    S: Subscriber,
    R: EventRecorder<Record: Clone> + 'static,
{
    fn new(buffer: usize) -> Self {
        let (tx, _) = broadcast::channel::<R::Record>(buffer.max(16));
        Self {
            _subscriber: PhantomData,
            channel: Arc::new(tx),
        }
    }
}

impl<S, R> Layer<S> for PublisherLayer<S, R>
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
        let record = R::record(event);
        let _ = self.channel.send(record);
    }
}

/// Publishing layer for tracing subscribers.
/// Publisher records events and publishes them to a broadcast channel.
///
/// # Type Parameters
/// - `S`: The tracing subscriber that accepts `Filtered` types as layers.
/// - `R`: The recorder type that filters and records events.
///   If not provided, `DefaultRecorder<PublisherDirective>` is used by default.
///
/// **Note**: Currently. this type uses the schema of the recorder as its message, and doesn't expect any exports.
/// It creates a default instance of `R::Schema`, and passes it to the recorder for updating fields.
pub struct EventPublisher<S, R = DefaultRecorder<PublisherDirective>>
where
    S: Subscriber,
    R: EventRecorder<Record: Clone>,
{
    inner: Filtered<PublisherLayer<S, R>, CallSiteFilter<S, R::Directive>, S>,
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
    /// > **Note**:
    /// > - Buffer size must be greater than `16`.
    /// > If value is less than `16`, it will be set to `16`.
    /// > - This instance can't be used as layer directly. Use `into_layer()` method to transform
    /// > it into `Filtered` layer after referencing its channel.
    ///
    /// # Example
    /// ```
    /// use tracing_subscriber::layer::SubscriberExt;
    /// use tracing_subscriber::Registry;
    ///
    /// use autonomic_events::layer::publisher::{EventChannel, EventPublisher};
    /// use autonomic_events::record::DefaultEvent;
    ///
    ///  // A new publisher with the default recorder and directive
    ///  let publisher = EventPublisher::<Registry>::new(16);
    ///  // A reference to channel before transforming publisher into layer
    ///  let channel = publisher.channel();
    ///  // Initialize the tracing subscriber with publisher as layer
    ///  let tracing_subscriber = Registry::default().with(publisher.into_layer());
    ///  // New subscriber to publisher's channel
    ///  let mut events_receiver = channel.subscribe();
    /// ```
    pub fn new(buffer: usize) -> Self {
        Self {
            inner: Filtered::new(PublisherLayer::new(buffer), CallSiteFilter::new()),
        }
    }

    /// Returns a reference to the channel for creating subscribers.
    ///
    /// Subscribers can be created from the channel using `subscribe()` method.
    ///
    /// > **Note**: Recording and publishing events will be **suspended**,
    /// > when there are **no active subscribers** to receive events.
    pub fn channel(&self) -> EventChannel<R::Record> {
        self.inner.inner().channel.clone()
    }

    /// Consumes the publisher and returns its `Layer` fused with its filter as `Filtered` layer.
    pub fn into_layer(self) -> Filtered<PublisherLayer<S, R>, CallSiteFilter<S, R::Directive>, S> {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tracing::subscriber;
    use tracing_core::Level;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use crate::record::DefaultEvent;
    use crate::{trace_error, trace_info};

    #[tokio::test]
    async fn test_publisher_publish_stop_on_error() {
        // -------------------- Shared Setup ----------------------

        // A new publisher with the default recorder and directive
        let publisher = EventPublisher::<Registry>::new(16);

        // A reference to channel before moving publisher
        let channel: EventChannel<DefaultEvent> = publisher.channel();

        // Initialize the tracing subscriber with publisher as layer
        let tracing_subscriber = Registry::default().with(publisher.into_layer());

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

        // Receive events
        let received_event1 = events_receiver.recv().await.unwrap();
        let received_event2 = events_receiver.recv().await.unwrap();

        assert_eq!(received_event1.tracing_level(), Level::INFO);
        assert_eq!(received_event1.source(), "published event 1");
        assert_eq!(received_event1.message(), "info message");

        assert_eq!(received_event2.tracing_level(), Level::ERROR);
        assert_eq!(received_event2.source(), "published event 2");
        assert_eq!(received_event2.message(), "error message");

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
