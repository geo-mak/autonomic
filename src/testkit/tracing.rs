use std::sync::Once;
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, Registry};

static INIT: Once = Once::new();

/// Initializes global default data subscriber with level `TRACE`.
/// This function will only be executed once, even if it is called multiple times.
pub fn init_tracing() {
    // Setting global default must be done only once, otherwise it will panic
    INIT.call_once(|| {
        let filter = tracing_subscriber::filter::EnvFilter::new("autonomic=trace");
        let layer = tracing_subscriber::fmt::layer().with_filter(filter);

        let subscriber = Registry::default().with(layer);
        set_global_default(subscriber).expect("Failed to set global data subscriber");
    });
}
