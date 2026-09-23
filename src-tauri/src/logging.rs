//! Logs go to the unified log (subsystem `com.himanshuat.openflow`, like the Swift app) and to stderr.
//! `log stream --predicate 'subsystem == "com.himanshuat.openflow"' --level info`

use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_target(true));
    #[cfg(target_os = "macos")]
    let registry = registry.with(tracing_oslog::OsLogger::new("com.himanshuat.openflow", "app"));
    let _ = registry.try_init();
}
