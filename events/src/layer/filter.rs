use std::marker::PhantomData;

use tracing_core::{Interest, Metadata, Subscriber};

use tracing_subscriber::layer::{Context, Filter};

use crate::traits::RecorderDirective;

/// A per-Layer filter that determines whether a span or event is enabled for an individual layer.
///
/// # Type Parameters
/// - `S`: The tracing subscriber type.
/// - `D`: The directive type that checks if the recorder is enabled based on the current metadata.
pub struct CallSiteFilter<S, D>
where
    S: Subscriber,
    D: RecorderDirective,
{
    _subscriber: PhantomData<S>,
    _directive: PhantomData<D>,
}

impl<S, D> CallSiteFilter<S, D>
where
    S: Subscriber,
    D: RecorderDirective,
{
    pub fn new() -> Self {
        Self {
            _subscriber: PhantomData,
            _directive: PhantomData,
        }
    }
}

impl<S, D> Filter<S> for CallSiteFilter<S, D>
where
    S: Subscriber,
    D: RecorderDirective,
{
    // Fallback method
    #[inline]
    fn enabled(&self, meta: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        D::enabled(meta)
    }

    // This method shall be called only once per call-site and result shall be cached by
    // the subscriber
    #[inline]
    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        if D::enabled(meta) {
            Interest::always()
        } else {
            Interest::never()
        }
    }
}
