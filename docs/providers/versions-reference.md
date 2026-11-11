# Versions reference

This page lists every field of `versions.json`, the file that lists the runtime artifacts a provider can install. The file uses schema `stackly.provider-versions/v1`. Your editor can check it against [`versions.v1.schema.json`](../../schemas/provider/versions.v1.schema.json); `cargo xtask provider check` runs the full rules below.

Do not write this file by hand for an official package: `cargo xtask codegen` writes it from `runtimes/inventory.toml` and from the evidence in `evidence/`, and `cargo xtask codegen --check` fails when it drifts. Only a package in the `testing` namespace, such as `providers/example/`, writes it by hand.

The other two files of a package have their own pages: [manifest-reference.md](manifest-reference.md) for `provider.yaml` and [settings-reference.md](settings-reference.md) for `schemas/settings.schema.json`.

## Root

The file is strict JSON with camelCase keys, at most 1 MiB. A key the loader does not know is `unknown_field`.

| Field | Required | Rules |
| --- | --- | --- |
| `schema` | yes | Exactly `stackly.provider-versions/v1` |
| `software` | yes | The same value as `metadata.software` in `provider.yaml` |
| `artifacts` | yes | The artifacts, see [artifacts](#artifacts) |

From `providers/example/versions.json`:

```json
{
  "schema": "stackly.provider-versions/v1",
  "software": "fake",
  "artifacts": [
    {
      "id": "fake-0.1.0-macos-aarch64",
      "line": "0.1",
      "version": "0.1.0",
      "os": "macos",
      "arch": "aarch64",
      "status": "fixture",
      "license": "MIT OR Apache-2.0",
      "executables": ["stackly-fake-server"]
    }
  ]
}
```

## artifacts

Every artifact has these fields:

| Field | Rules |
| --- | --- |
| `id` | Unique in the file; letters, digits, `.`, `_`, `-` |
| `line` | The runtime line, such as `"16"` or `"2.15"` |
| `version` | The exact version, which starts with the line and a dot, such as `"2.15.1"` |
| `os` | Today `macos` |
| `arch` | Today `aarch64` |
| `status` | `available`, `blocked`, or `fixture`; it decides which other fields the artifact must and must not have |

The pair of `version`, `os`, and `arch` is unique in the file.

### Status available

An artifact a user can install. It must have:

| Field | Rules |
| --- | --- |
| `minOs` | The oldest macOS version that runs it, such as `"13.0"` |
| `format` | `tar_gz`, `zip`, or `binary` |
| `urls` | The download URLs |
| `sha256` | The SHA-256 of the download |
| `size` | The size of the download in bytes |
| `signingPolicy` | `keep_or_adhoc` or `adhoc_pipeline` |
| `license` | The SPDX license of the runtime, such as `Apache-2.0` |
| `licenseUrl` | Where the license text lives |
| `executables` | Paths of the executables inside the runtime; `runtime.executable` and `permissions.additionalExecutables` must be among them |

It may also have:

| Field | Rules |
| --- | --- |
| `archiveRoot` | The one folder at the root of the archive |
| `stable` | Only `true`; at most one stable artifact per `os` and `arch`. `@stable` picks it. |
| `build` | A build number from 1, for an artifact maintainers build |
| `unpackedSize` | Bytes after unpacking; present together with `entryCount` |
| `entryCount` | Entries in the archive; present together with `unpackedSize` |
| `dependencies` | The libraries it links, see [dependencies](#dependencies) |
| `compatibility` | Test records, at most 32, see [compatibility](#compatibility) |

From `providers/nats/versions.json`, shortened:

```json
{
  "id": "nats-2.15.1-macos-aarch64",
  "line": "2.15",
  "version": "2.15.1",
  "os": "macos",
  "arch": "aarch64",
  "status": "available",
  "minOs": "13.0",
  "format": "tar_gz",
  "archiveRoot": "nats-server-v2.15.1-darwin-arm64",
  "signingPolicy": "keep_or_adhoc",
  "license": "Apache-2.0",
  "executables": ["nats-server"],
  "stable": true
}
```

### Status blocked

An artifact the catalog shows but nobody can install yet. It must have `blocked` with three fields:

| Field | Rules |
| --- | --- |
| `reason` | Why it is blocked |
| `owner` | Who unblocks it |
| `since` | The date it was blocked, `YYYY-MM-DD` |

It may have `license`, `licenseUrl`, and `build`, and no other artifact field.

### Status fixture

An artifact that runs a test server from this repo, such as `stackly-fake-server`. Only a package in the `testing` namespace may use it (`fixture_outside_testing` otherwise). It must have `executables`, may have `license` and `licenseUrl`, and must not have `urls`, `sha256`, `size`, `format`, `minOs`, `signingPolicy`, `blocked`, or any optional field of an available artifact.

## dependencies

One entry per library an available artifact links. `name` is unique.

| Field | Required | Rules |
| --- | --- | --- |
| `name` | yes | The library file name |
| `kind` | yes | `bundled` (shipped inside the runtime), `own` (built with the artifact from its own source), or `system` (from macOS) |
| `installName` | yes | The install name the binary records, 1 to 1024 bytes |
| `version` | for `bundled` | The library version |
| `license` | for `bundled` | Its SPDX license |
| `sourceUrl` | for `bundled` | An `https` URL of its source |

From `providers/nats/versions.json`:

```json
{
  "name": "libSystem.B.dylib",
  "kind": "system",
  "installName": "/usr/lib/libSystem.B.dylib"
}
```

## compatibility

`cargo xtask codegen` writes these records from native evidence; never write them yourself. The key of a record is its `scope`, its `providerRevision`, the major part of `osVersion`, and `arch`, and each key appears once.

| Field | Required | Rules |
| --- | --- | --- |
| `scope` | yes | `runtime` (the runtime alone was tested) or `provider` (a provider revision was tested on it) |
| `result` | yes | `pass` or `fail` |
| `providerRevision` | with scope `provider` | `{ id, revision, manifestSha256 }` of the tested package; `null` for scope `runtime` |
| `os` | yes | Today `macos` |
| `osVersion` | yes | The macOS version of the test, 1 to 16 bytes, such as `"27.0"` |
| `osBuild` | no | The macOS build, 1 to 32 letters or digits |
| `arch` | yes | Today `aarch64` |
| `checks` | yes | At most 64 snake case check codes that ran |
| `knownLimitations` | yes | At most 64 snake case codes of known limits |
| `summary` | no | At most 256 characters |
| `testedAt` | yes | UTC time in RFC 3339 |
| `evidence` | yes | Path of the evidence record under `evidence/` |

`providerRevision` fields:

| Field | Rules |
| --- | --- |
| `id` | The `metadata.id` of the package |
| `revision` | Its `metadata.revision` |
| `manifestSha256` | The package digest: one SHA-256 over `provider.yaml`, `schemas/settings.schema.json`, and every template the package uses |

From `providers/postgresql/versions.json`, shortened:

```json
{
  "scope": "runtime",
  "result": "pass",
  "providerRevision": null,
  "os": "macos",
  "osVersion": "27.0",
  "osBuild": "26A428",
  "arch": "aarch64",
  "checks": ["archive_report", "closure", "dependencies"]
}
```
