# official/nats

Runs `nats-server` with one rendered `nats.conf` per instance (spec 0009):
the client listener and the HTTP monitoring listener on loopback, and
JetStream storing into the instance's data folder (`<data>/jetstream`).

## Client password

Every instance created from revision `1.1.0` asks for a password. Clients
sign in as the user `stackly` with the instance's `clientPassword`, which
Stackly generates at create (or takes from you) and keeps in the Keychain.
The Client URL in the `connection` block carries both:
`nats://stackly:<password>@<address>`, shown masked until you reveal or copy
it.

To change the password, set a new value for the secret reference of the
`clientPassword` field with `stackly secret set`, then restart the instance.
The new value takes effect at that restart; no config revision is written,
because the settings only hold the reference.

### Connections without credentials

The server also has a user `stackly-readiness` without a password, in its
own NATS account `STACKLY_READINESS`, and maps every connection that sends
no credentials to it (`no_auth_user`). Stackly's readiness probe uses it, so
the probe never needs the password. That user can only send `PING`, ask
`$JS.API.INFO` when JetStream is on, and subscribe to `_INBOX.stackly.>`. It
never sees a subject or a stream of the `stackly` user.

So a client that forgets its credentials still connects, and then gets a
`Permissions Violation` at its first publish or subscribe, not an
`Authorization Violation` at connect. If you see `Permissions Violation`,
check that your client uses the Client URL with the user and password.

## Where nats.conf lives

`nats.conf` holds the password, so it is a sensitive config (spec 0011): it
is written to the runtime `secrets/` folder of the instance with mode `0600`
right before every start, and removed after a clean stop. It exists only
while the instance runs, it is never in the config folder or its history,
and it does not show up as an Effective File in the Configuration tab.

## Monitoring

The monitoring listener (`/varz`, `/connz`, `/jsz`, `/accountz`, `/subsz`)
has no authentication in NATS, so any local process can read it. It is bound
to loopback only. It shows statistics, account and user names, stream names,
and subscribed subjects; it never shows the password or message contents.

## JetStream durability

File streams survive a normal restart of the instance. Memory streams do
not: they are gone after every restart. After a crash or a power loss the
last writes to a file stream can be lost, because the server flushes to disk
on an interval.

## Limits in MiB and KiB

The JetStream memory and file limits are set in MiB, and the largest message
in KiB. Both are binary units: `1024` MiB is 1073741824 bytes.

## Readiness

Readiness uses the native `nats-readiness` probe (spec 0023), which speaks
the NATS client protocol to the `client` endpoint instead of trusting an open
port. One attempt checks that:

- the server that answers reports the `server_name` of this instance
  (`serverName` is the instance name), so another process on the port is
  never taken for this one;
- JetStream is on or off as the instance's `jetstream` setting asks;
- the server answers a `PING` with `PONG`;
- when JetStream is on, the `$JS.API.INFO` API answers without an error.

The probe connects as `stackly-readiness` about every 10 seconds, sends no
credentials, and closes the connection when the attempt ends.

## Instances from revision 1.0.1

An instance created from revision `1.0.1` keeps that revision and stays
without authentication. Duplicating it makes another instance of `1.0.1`,
again without authentication, because a duplicate uses the revision of its
source. To get a password, create a new instance.

## Limits of this version

- A password shaped like a bcrypt hash (`$2a$`, `$2b$`, `$2x$`, or `$2y$`
  followed by two digits and `$`) cannot be used: NATS reads it as a hash,
  so a login with that text is refused. Passwords Stackly generates only use
  letters and digits and never have this shape.
- Readiness does not notice a full JetStream store: the server stays up and
  answers the probe while streams fail to write.

`versions.json` is generated from `runtimes/inventory.toml` by
`cargo xtask codegen`; never edit it by hand.
