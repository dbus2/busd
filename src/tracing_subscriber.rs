pub fn init() {
    #[cfg(all(feature = "tracing-subscriber", not(feature = "console-subscriber")))]
    {
        use tracing_subscriber::{util::SubscriberInitExt, EnvFilter, FmtSubscriber};

        FmtSubscriber::builder()
            .with_env_filter(EnvFilter::from_default_env())
            .finish()
            .init();
    }

    #[cfg(feature = "console-subscriber")]
    console_subscriber::init();
}
