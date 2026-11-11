# Security policy

## Reporting a vulnerability

Report a vulnerability privately with the **Report a vulnerability** button on the Security tab of this repository: <https://github.com/stackly-dev/stackly/security/advisories/new>. Only you and the repository admins can see the report.

Please do not open an issue, a pull request, or a discussion about a vulnerability. Public details before a fix is out put every Stackly user at risk.

## What to include

- The Stackly version (`stackly --version`) and the macOS version.
- The provider and the runtime version involved, if any.
- Steps to reproduce, as small as you can make them.
- The impact: what an attacker can read, change, or run.
- Whether anyone else already knows about it.

A diagnostic bundle (`stackly diagnostics export`) is already redacted, but please read it before you attach it. Never send a password or another secret, even one from a test setup.

## Scope

In scope:

- `stacklyd`, the per user daemon.
- `stackly`, the command line client.
- The macOS app.
- The IPC protocol between them.
- The provider packages in `providers/` and the bundled catalog.
- The packaging of runtime artifacts that Stackly maintainers build.

Out of scope:

- A vulnerability in the upstream software itself (PostgreSQL, NATS, Traefik, and the others). Please report it to the upstream project, and tell us too, so we can pin a fixed version or pull an affected one.
- Attacks that need a process already running as the same macOS user, or as root.

Stackly trusts one macOS user account: the daemon, the client, and the app all run as that user, and secrets live only in that user's Keychain. A report outside that boundary is still welcome through the private channel, and we treat it as hardening.

## Supported versions

Until the first release, only `main` gets security fixes. After that, and before 1.0, the latest release and `main` get them.

## What to expect

- We confirm that we received your report within 7 days.
- We send a first assessment within 14 days.
- We aim to fix or publish within 90 days of your report, and we tell you if it needs longer.

These are targets, not promises: one maintainer runs this project today.

## Our process

1. The security contact confirms the report and reproduces it in a test root, never in a real Stackly setup.
2. The security contact opens a draft GitHub Security Advisory and invites you to it.
3. The fix is built in the temporary private fork of that advisory, with the same local checks as any pull request.
4. For a problem in a provider package, a new package revision is published and the affected revision is pulled from the bundled catalog. When users may already have it, the affected revision is also added to the catalog's `revoked` list, which reaches them with the fixed release: Stackly then refuses new installs and instances from it, and existing instances and their data stay. For a runtime artifact, a new build gets a new inventory entry.
5. The fix is released, then the advisory is published. When a released version is affected, we request a CVE through GitHub.
6. You are credited in the advisory, unless you ask us not to.
7. `CHANGELOG.md` gets an entry under `Security`.

No sensitive detail is made public before step 5.
