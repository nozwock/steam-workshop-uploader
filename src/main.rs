use std::path::PathBuf;

use eyre::eyre::Result;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use tracing_utils::{format::SourceFormatter, writer::RotatingFileWriter};

fn main() -> Result<()> {
    let (non_blocking, _guard) = tracing_appender::non_blocking(RotatingFileWriter::new(
        3,
        PathBuf::from(".").canonicalize()?,
        "workshop-uploader.log",
    )?);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(SourceFormatter)
                .with_writer(non_blocking),
        )
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::DEBUG.into())
                .from_env_lossy(),
        )
        .init();

    Ok(())
}
