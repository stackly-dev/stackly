# TLS test fixtures

`cert.der` and `key.der` are a self signed certificate for `localhost` and
`127.0.0.1` (EC P-256, valid 800 days) with its private key, made once with
`openssl` for `tests/fetch_tls.rs`. They exist only to prove that
`MacosFetcher` refuses a certificate the macOS trust store does not know. The
key protects nothing and is not a credential.
