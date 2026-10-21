//! `MacosFetcher` and certificates (spec 0018, AC-5): a certificate the
//! macOS trust store does not know is refused as `Tls`, and the plain HTTP
//! mapping of the test feature is not used here, so the real verifier runs.

use std::net::TcpListener;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection};
use stackly_platform::{FetchError, FetchRequest, FetchResponse, FetchSink, Fetcher, HttpsUrl};
use stackly_platform::{SinkError, StopFlag};
use stackly_platform_macos::MacosFetcher;

const CERT: &[u8] = include_bytes!("fixtures/tls/cert.der");
const KEY: &[u8] = include_bytes!("fixtures/tls/key.der");

struct Nothing;

impl FetchSink for Nothing {
    fn start(&mut self, _: &FetchResponse) -> Result<(), SinkError> {
        Ok(())
    }

    fn write(&mut self, _: &[u8]) -> Result<(), SinkError> {
        Ok(())
    }
}

/// Serves a TLS handshake with the self signed fixture certificate.
fn tls_server() -> u16 {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("the default protocol versions are valid")
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(CERT.to_vec())],
            PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
        )
        .expect("the fixture key matches the fixture certificate");
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port is free");
    let port = listener.local_addr().expect("a bound listener").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut tcp) = stream else { continue };
            let mut conn = ServerConnection::new(Arc::clone(&config)).expect("a server connection");
            // The client refuses the certificate, so this ends in an alert.
            let _ = conn.complete_io(&mut tcp);
        }
    });
    port
}

#[test]
fn a_self_signed_certificate_is_refused_as_a_tls_failure() {
    let port = tls_server();
    let url = HttpsUrl::new(&format!("https://127.0.0.1:{port}/artifact.tar.gz")).expect("a url");
    let stop = StopFlag::new();
    let request = FetchRequest {
        url: &url,
        resume: None,
        size: 10,
        stop: &stop,
    };
    let fetcher = MacosFetcher::with_timeouts(Duration::from_secs(5), Duration::from_secs(5));

    let result = fetcher.fetch(&request, &mut Nothing);

    assert!(
        matches!(result, Err(FetchError::Tls { .. })),
        "a self signed certificate must be a Tls error: {result:?}"
    );
}
