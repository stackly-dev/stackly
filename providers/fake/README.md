# testing/fake

The fake provider runs `stackly-fake-server` (`crates/fake-server`) for the
supervisor, walking skeleton, and engine tests (spec 0009). Its namespace is
`testing`, so it never ships in the app catalog. `versions.json` is written by
hand: its one `fixture` artifact has the version of the `stackly-fake-server`
crate.
