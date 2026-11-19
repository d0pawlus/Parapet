//! One HTTP client setup for everything that talks to the internet (weather, update checks).

use std::time::Duration;

pub const USER_AGENT: &str = concat!("Parapet/", env!("CARGO_PKG_VERSION"));

/// An agent with the system TLS stack and a global timeout for each request.
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(USER_AGENT)
        .tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build())
        .build()
        .into()
}

/// A short, user-facing description of a failed request.
pub fn describe_error(url: &str, e: ureq::Error) -> String {
    let host = url.split('/').nth(2).unwrap_or(url);
    match e {
        ureq::Error::StatusCode(code) => format!("HTTP {code}"),
        _ => format!("cannot connect to {host}"),
    }
}
