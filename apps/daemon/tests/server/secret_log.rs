//! The daemon's log writer against values the daemon learned through its
//! own `secret.*` methods (spec 0011 AC-13): a value that was set, one that
//! was replaced, and one the daemon generated are all scrubbed from a line
//! written at any level, in a message, in a field, and in a span field.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use stackly_core::secret::{SecretWorker, WorkerConfig};
use stackly_platform::SecretStore;
use stackly_protocol::events::ShutdownReason;
use stackly_protocol::secret::{
    RevealPurpose, RevealSecretParams, SecretInput, SecretMode, SecretText, SetSecretParams,
};
use stackly_protocol::{Call, Reply, Revision};
use stackly_testkit::FakeSecretStore;
use stackly_testkit::evidence::{FixtureCredential, Kind, evidence};
use stacklyd::log_sink::RedactingMakeWriter;
use tracing::{Level, debug, error, info, info_span, trace, warn};

use crate::common::{Site, client};

const ADMIN: &str = "local:erp/postgres/admin";
const GENERATED: &str = "local:erp/postgres/generated";

/// Collects whole lines, redacted by the writer in front of it.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Write for Lines {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn set(reference: &str, input: SecretInput, mode: SecretMode, expected: Option<u64>) -> Call {
    Call::SecretSet(SetSecretParams {
        reference: reference.to_owned(),
        input,
        mode,
        expected_revision: expected.map(Revision::saturating),
        confirm_stored_only: false,
    })
}

#[tokio::test]
async fn values_the_daemon_learned_are_scrubbed_from_a_log_line_at_every_level() {
    let ev = evidence!(
        covers = ["REQ-016", "BR-018"],
        kind = Kind::Fixture,
        expected = "a value set, a value replaced, and a value the daemon generated through the secret methods of a running daemon are scrubbed by the daemon's redacting log writer from lines written at error, warn, info, debug, and trace, in a message, a field, and a span field, while a line without a secret passes whole",
    );
    let store = FakeSecretStore::new();
    let worker = SecretWorker::spawn(Arc::new(store), WorkerConfig::default())
        .expect("the worker thread starts");
    let site = Site::new().with_secret_store(worker as Arc<dyn SecretStore>);
    let daemon = site.start().await;
    let (client, _) = client(&site).await;
    let old = FixtureCredential::new();
    let current = FixtureCredential::new();
    for call in [
        set(
            ADMIN,
            SecretInput::Value(SecretText::new(old.value().to_owned())),
            SecretMode::Create,
            None,
        ),
        set(
            ADMIN,
            SecretInput::Value(SecretText::new(current.value().to_owned())),
            SecretMode::Replace,
            Some(1),
        ),
        set(GENERATED, SecretInput::Generate, SecretMode::Create, None),
    ] {
        client.call(call).await.expect("the secret is stored");
    }
    let Ok(Reply::SecretReveal(generated)) = client
        .call(Call::SecretReveal(RevealSecretParams {
            reference: GENERATED.to_owned(),
            purpose: RevealPurpose::Show,
        }))
        .await
    else {
        panic!("the generated value is revealed");
    };
    let generated = generated.value.expose().to_owned();

    let lines = Lines::default();
    let writer = {
        let lines = lines.clone();
        RedactingMakeWriter::new(move || lines.clone(), Arc::clone(daemon.core().redactor()))
    };
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_max_level(Level::TRACE)
        .with_writer(writer)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        for value in [old.value(), current.value(), generated.as_str()] {
            let span = info_span!("job", detail = value);
            let _entered = span.enter();
            error!(secret = value, "failed with {value} inside");
            warn!(secret = value, "warned about {value}");
            info!(secret = value, "info {value}");
            debug!(secret = value, "debug {value}");
            trace!(secret = value, "trace {value}");
            info!("connect postgres://admin:{value}@localhost/db");
            info!("POSTGRES_PASSWORD={value}");
        }
        info!("nothing secret here");
    });
    daemon
        .shutdown(ShutdownReason::Requested, std::future::pending())
        .await;

    let written = String::from_utf8_lossy(&lines.0.lock().unwrap_or_else(PoisonError::into_inner))
        .into_owned();
    old.assert_absent(&written, "a log line");
    assert!(
        !written.contains(&generated),
        "the generated value is in a log line"
    );
    assert!(written.contains("****"), "{written}");
    assert!(written.contains("nothing secret here"), "{written}");
    ev.pass();
}
