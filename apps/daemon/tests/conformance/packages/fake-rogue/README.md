# testing/fake-rogue

A copy of `providers/example` with three boolean settings, `closeLockFd`,
`reExec`, and `background`, passed to `stackly-fake-server serve` as the
flags that break the process contract of spec 0010 on purpose. Only the
conformance harness tests read it (spec 0036, AC-10); it never ships in a
catalog.
