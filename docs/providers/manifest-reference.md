# Manifest reference

This page lists every field of `provider.yaml`, the manifest of a provider package. The manifest uses `apiVersion: stackly/v1alpha1`. Your editor can check a manifest against the generated JSON Schema [`provider.v1alpha1.schema.json`](../../schemas/provider/provider.v1alpha1.schema.json); `cargo xtask provider check` runs the full loader, which also checks the rules the JSON Schema cannot express (names that must match, tokens per position, cross field rules).

The other two files of a package have their own pages: [settings-reference.md](settings-reference.md) for `schemas/settings.schema.json` and [versions-reference.md](versions-reference.md) for `versions.json`.

Every example on this page is cut from a real package in [`providers/`](../../providers/), and names its source file.

## How the loader reads a manifest

- The file is YAML with camelCase keys, at most 256 KiB. Every object is strict: a key the loader does not know is an `unknown_field` issue with its path.
- `apiVersion` is read first. A value this build does not support gets one `unsupported_api_version` issue, and nothing else of the file is decoded. Today the only supported version is `stackly/v1alpha1` (`SUPPORTED_API_VERSIONS` in `crates/core/src/provider/check.rs`).
- Names, versions, and enum values are read as plain text first and checked afterwards, so a wrong value gets an issue code with its path instead of a parse error.
- Many string fields take tokens such as `${instance.data}` or `${endpoint.client.port}`. Which tokens a field accepts is written in its row below. `$${` writes a literal `${`.

## Top level

| Field | Required | What it holds |
| --- | --- | --- |
| `apiVersion` | yes | Exactly `stackly/v1alpha1` |
| `kind` | yes | Exactly `Provider` |
| `metadata` | yes | Identity of the package, see [metadata](#metadata) |
| `compatibility` | yes | Platforms and data compatibility, see [compatibility](#compatibility) |
| `runtime` | yes | The main executable, see [runtime](#runtime) |
| `capabilities` | no | What the provider can do, see [capabilities](#capabilities) |
| `resources` | yes | The endpoints, see [resources](#resources) |
| `configFiles` | no | Config files rendered from templates, see [configFiles](#configfiles) |
| `lifecycle` | yes | Init, start, stop, and reload, see [lifecycle](#lifecycle) |
| `readiness` | yes | How Stackly knows the instance is ready, see [readiness and health](#readiness-and-health) |
| `health` | no | The probe that runs while the instance is up |
| `timeouts` | no | Time limits per phase, see [timeouts](#timeouts) |
| `logs` | no | Extra log files, see [logs](#logs) |
| `backup` | no | What a cold backup copies, see [backup](#backup) |
| `permissions` | yes | Which instance folders and executables the provider uses, see [permissions](#permissions) |
| `connection` | no | Connection strings, see [connection](#connection) |
| `openWith` | no | Actions that open an instance in another app, see [openWith](#openwith) |
| `remedies` | no | Repair steps the doctor shows, see [remedies](#remedies) |

From `providers/postgresql/provider.yaml`:

```yaml
apiVersion: stackly/v1alpha1
kind: Provider
metadata:
  id: official/postgresql
  software: postgresql
  displayName: PostgreSQL
  revision: "1.0.2"
  category: database
```

## metadata

| Field | Required | Rules |
| --- | --- | --- |
| `id` | yes | `<namespace>/<name>`, each part lowercase letters, digits, and single `-`, starting with a letter, at most 127 characters in all. The name part must equal the package folder name. |
| `software` | yes | The software this package runs, the same rules as one part of `id`, at most 63 characters. It must equal `software` in `versions.json`. |
| `displayName` | yes | 1 to 64 characters, no control characters |
| `revision` | yes | The package revision, at most 64 characters, such as `"1.0.2"`. Quote it in YAML so it stays text. Change it whenever a file that counts toward the package digest changes. |
| `category` | yes | One of `database`, `messaging`, `cache`, `proxy`, `analytics`, `testing` |
| `description` | no | At most 512 characters, no control characters |

## compatibility

| Field | Required | Rules |
| --- | --- | --- |
| `platforms` | yes | At least one pair of `os` and `arch`, each pair unique. Today the only pair is `os: macos` with `arch: aarch64`. |
| `lineCompatibility` | no | A map from a runtime line, such as `"16"` or `"2.15"`, to other lines that may run data written by that line. A line can always run its own data. Empty means strict: only the same line. |
| `dataMarker` | no | `{ file, pattern }`: a file in the data folder that records which line wrote the data. `file` is one path component, such as `PG_VERSION`. `pattern` follows the settings pattern rules and has exactly one capture group, which must give a runtime line. |

From `providers/postgresql/provider.yaml`:

```yaml
compatibility:
  platforms:
    - { os: macos, arch: aarch64 }
  dataMarker: { file: PG_VERSION, pattern: "([0-9]+)" }
```

## runtime

| Field | Required | Rules |
| --- | --- | --- |
| `executable` | yes | Path of the main executable inside the runtime, made of literal components (`A` to `Z`, `a` to `z`, digits, `.`, `_`, `-`), never `..`. It must be listed in `executables` of the artifact in `versions.json`. |
| `executionMode` | yes | Only `foreground`: the main process stays in the foreground, never re executes itself, and keeps the file descriptors it inherits (the instance lock is fd 3). |
| `versionCheck` | no | How the installer reads the version of a runtime once, see below |

`versionCheck` fields:

| Field | Required | Rules |
| --- | --- | --- |
| `args` | yes | At most 8 literal strings; no tokens |
| `pattern` | yes | A regular expression, at most 512 characters, with exactly one capture group. It is matched line by line against stdout, then stderr, and is not anchored for you. |
| `timeoutSeconds` | no | 1 to 60, default 15 |

From `providers/nats/provider.yaml`:

```yaml
runtime:
  executable: nats-server
  executionMode: foreground
  versionCheck:
    args: ["--version"]
    pattern: '^nats-server: v([0-9]+\.[0-9]+\.[0-9]+)$'
    timeoutSeconds: 10
```

## capabilities

Every field is optional.

| Field | Default | Meaning and rule |
| --- | --- | --- |
| `reload` | `false` | The instance can apply some settings without a restart. Needs `lifecycle.reload`, and `lifecycle.reload` needs this. |
| `metrics` | `false` | Needs an endpoint with `purpose: monitoring` |
| `multiInstance` | `false` | More than one instance of this provider may exist |
| `persistence` | `false` | The instance keeps data between starts |
| `tls` | `false` | Not supported in `stackly/v1alpha1`; `true` is rejected |
| `backup` | `[]` | A list of backup kinds. `cold` needs `persistence: true` and `backup.cold.paths`. `online` and `logical` are not supported in `stackly/v1alpha1` and are rejected. |

## resources

`endpoints` is a map from an endpoint name to one endpoint, with 1 to 16 entries. A name is a lowercase letter followed by up to 31 lowercase letters or digits.

| Field | Required | Rules |
| --- | --- | --- |
| `transport` | yes | `tcp`, `udp`, or `unix` |
| `purpose` | yes | `client`, `monitoring`, or `admin` |
| `defaultHost` | for `tcp` and `udp` | A loopback address, such as `"127.0.0.1"` |
| `defaultPort` | for `tcp` and `udp` | 1 to 65535. It is the suggestion the create form shows; the user picks a fixed or automatic port. |
| `socketName` | for `unix` | The socket file name: literal text from `A` to `Z`, `a` to `z`, digits, `.`, `_`, `-`, plus the token `${endpoint.<tcp endpoint>.port}` |

From `providers/postgresql/provider.yaml`:

```yaml
resources:
  endpoints:
    client: { transport: tcp, purpose: client, defaultHost: "127.0.0.1", defaultPort: 5432 }
    socket: { transport: unix, purpose: client, socketName: ".s.PGSQL.${endpoint.client.port}" }
```

## configFiles

A map from a config file name to one config file, at most 16 entries. A name is a lowercase letter followed by up to 31 letters or digits. Every template in `templates/` must be used by one entry here.

| Field | Required | Rules |
| --- | --- | --- |
| `template` | yes | `templates/<file>.tmpl`; the file name is lowercase letters, digits, `.`, `_`, `-` |
| `format` | yes | How values are escaped when the template is rendered: `yaml`, `json`, `natsConf`, `postgresqlConf`, `plain` (no value tags at all; the file is written as it is), or `valkeyConf` (text in double quotes with `\` and `"` escaped, booleans `yes` and `no`) |
| `destination` | yes | Where the rendered file goes: one folder token at the start, then one or two literal components, such as `"${instance.config}/nats.conf"` |
| `sensitive` | no | Default `false`. `true` means the file holds a secret; its destination must then start with `${instance.secrets}`, and only a sensitive template may write `secret.<field>` values. |

From `providers/traefik/provider.yaml`:

```yaml
configFiles:
  staticConfig:
    template: templates/traefik.yaml.tmpl
    format: yaml
    destination: "${instance.config}/traefik.yaml"
  routes:
    template: templates/routes.yaml.tmpl
    format: yaml
    destination: "${instance.config}/dynamic/routes.yaml"
```

## lifecycle

| Field | Required | What it holds |
| --- | --- | --- |
| `initialize` | no | At most 16 steps that run once when an instance is created |
| `start` | yes | The command that runs the main process |
| `stop` | yes | How the main process is stopped |
| `reload` | when `capabilities.reload` is `true` | How changed settings are applied without a restart |

### Commands

`start`, an `exec` probe, a stop `command`, and a reload `command` use the same command shape.

| Field | Required | Rules |
| --- | --- | --- |
| `executable` | yes | Exactly one executable token and no other text: `${runtime.executable}` or `${runtime.executables.<alias>}` |
| `args` | no | At most 128 strings, one argv element each. An entry that starts with a user setting that could become a new flag is rejected (`argument_injection_risk`). |
| `cwd` | no | One folder token at the start, then literal components. Default `${instance.data}`. |
| `env` | no | At most 32 variables. A name is uppercase letters, digits, and `_`, starting with a letter; never `PATH`, `HOME`, `TMPDIR`, or a name that starts with `DYLD_` or `LD_`. A value may be one whole `${secret.<field>}` token only in `lifecycle.start` and in a `runExecutable` step. |

### Initialize steps

Each step names a `primitive`, which decides which other fields it takes.

| `primitive` | Fields | What it does |
| --- | --- | --- |
| `createManagedDirectories` | none | Makes sure every folder in `permissions.managedFilesystem` exists |
| `renderConfig` | `file` | Renders the config file with that name from `configFiles` |
| `materializeSecret` | `secret`, `destination` | Writes the bytes of the secret field `secret` to a file named `destination` (one literal component) in `${instance.secrets}`, mode 0600, removed at the end of the init. Needs `run` in `managedFilesystem`. |
| `runExecutable` | the command fields, `stdin`, `timeoutSeconds`, `successExitCodes` | Runs a runtime executable |

`runExecutable` fields beyond the command fields:

| Field | Required | Rules |
| --- | --- | --- |
| `stdin` | no | Exactly one of `secret` (the exact bytes of that secret field) or `text` (1 to 16384 bytes of literal UTF-8, no tokens, no control characters except newline and tab). stdin is closed afterwards. |
| `timeoutSeconds` | no | At most `timeouts.initSeconds`, which is also the default |
| `successExitCodes` | no | 1 to 8 values from 0 to 255, default `[0]` |

From `providers/postgresql/provider.yaml`, shortened:

```yaml
lifecycle:
  initialize:
    - { primitive: createManagedDirectories }
    - { primitive: renderConfig, file: pgHba }
    - { primitive: materializeSecret, secret: superuserPassword, destination: superuser-password }
    - primitive: runExecutable
      executable: "${runtime.executables.initdb}"
      args:
        - "--pgdata=${instance.data}"
        - "--pwfile=${instance.secrets}/superuser-password"
      cwd: "${instance.temp}"
      timeoutSeconds: 90
```

### stop

| Field | Rules |
| --- | --- |
| `strategy` | `gracefulProcess` sends `signal` to the main process; `command` runs `command` and then waits for the main process to exit. There is no kill strategy. |
| `signal` | With `gracefulProcess`: `term` (default), `int`, or `quit` |
| `command` | With `command`: the stop command, in the command shape above (no secret in `env`) |
| `successExitCodes` | With `command`: default `[0]` |

### reload

| Field | Rules |
| --- | --- |
| `render` | Names from `configFiles` to render again, at most 16, may be empty |
| `action` | `signal` (sends `signal: hup`), `run` (runs `command` with `successExitCodes`, default `[0]`), or `none` (the server watches its files) |
| `signal` | With `action: signal`: `hup` |
| `command` | With `action: run`: the reload command |
| `successExitCodes` | With `action: run`: default `[0]` |
| `verify` | `readiness` (default: the readiness probe must pass again) or `none` |

From `providers/postgresql/provider.yaml`:

```yaml
  stop: { strategy: gracefulProcess, signal: int }
  reload:
    render: [pgHba, pgIdent, postgresqlConf]
    action: signal
    signal: hup
    verify: readiness
```

## readiness and health

`readiness` decides when a started instance is ready. `health` keeps checking while it runs; without its own `probe` it uses the readiness probe.

| Block | Field | Rules |
| --- | --- | --- |
| `readiness` | `probe` | Required, see [probes](#probes) |
| `readiness` | `intervalMilliseconds` | 100 to 10000, default 500: the pause after one attempt ends |
| `readiness` | `attemptTimeoutMilliseconds` | 100 to 30000, default 2000 |
| `health` | `probe` | Optional |
| `health` | `intervalSeconds` | 1 to 300, default 10 |
| `health` | `attemptTimeoutMilliseconds` | 100 to 30000, default 2000 |
| `health` | `failureThreshold` | 1 to 10, default 3. Failures in a row below it mean `degraded`; reaching it means `unhealthy`. |

### Probes

`type` picks which other fields a probe takes; a field of another type is rejected.

| `type` | Fields | Passes when |
| --- | --- | --- |
| `tcp` | `endpoint` (a `tcp` or `unix` endpoint) | A connection opens within the attempt timeout |
| `http` | `endpoint` (a `tcp` endpoint), `path` (literal, starts with `/`, at most 256 printable ASCII bytes without spaces), `expectStatus` (`min` and `max` within 100 to 599, default 200 to 299) | A `GET` gets a status inside `expectStatus`; redirects are not followed |
| `exec` | `command` (the command shape, no stdin), `successExitCodes` (default `[0]`) | The command exits with one of `successExitCodes` |
| `native` | `name`, `endpoint`, `params` | A probe built into Stackly passes. The names are `nats-readiness` (params `serverName`, `jetstream`) and `traefik-routes` (param `routesFile`). `params` holds at most 8 scalar values, each checked against what that probe expects. |

From `providers/example/provider.yaml`:

```yaml
readiness:
  probe: { type: http, endpoint: http, path: /ready }
health:
  probe: { type: http, endpoint: http, path: /health }
```

From `providers/nats/provider.yaml`:

```yaml
readiness:
  probe:
    type: native
    name: nats-readiness
    endpoint: client
    params:
      serverName: "${settings.serverName}"
      jetstream: "${settings.jetstream}"
```

## timeouts

Every field is optional and counts seconds.

| Field | Default | Range | Covers |
| --- | --- | --- | --- |
| `initSeconds` | 120 | 1 to 1800 | The whole initialize |
| `readinessSeconds` | 60 | 1 to 600 | From spawn until ready on a start |
| `recoverySeconds` | same as `readinessSeconds` | 1 to 1800 | From a respawn after a crash until ready |
| `stopSeconds` | 30 | 1 to 600 | From the stop action until the process exits |
| `reloadSeconds` | 30 | 1 to 300 | The reload action and its verify; only with `capabilities.reload` |

A manifest cannot set the restart policy; the daemon owns it.

## logs

`files` lists at most 8 extra log files. Each starts with `${instance.logs}` followed by literal components, and `logs` must be in `permissions.managedFilesystem`.

## backup

`cold` holds `paths`, a list of `data` and `config`, each also in `permissions.managedFilesystem`. The block is required exactly when `capabilities.backup` holds `cold`.

From `providers/nats/provider.yaml`:

```yaml
backup:
  cold: { paths: [data] }
```

## permissions

| Field | Required | Rules |
| --- | --- | --- |
| `managedFilesystem` | yes | The instance folders this provider uses, from `data`, `config`, `logs`, `temp`, `run`. Every folder token anywhere in the package must be listed. `${instance.secrets}`, `materializeSecret`, and unix sockets need `run`; an `openWith` entry of kind `terminal` needs `temp`. |
| `executeRuntimeBinary` | yes | Must be `true`: a plain statement that the provider runs its runtime |
| `additionalExecutables` | no | At most 16 aliases, each a path inside the runtime that is listed in `executables` of the artifact. A command names one as `${runtime.executables.<alias>}`. |

From `providers/postgresql/provider.yaml`:

```yaml
permissions:
  managedFilesystem: [data, config, temp, run]
  executeRuntimeBinary: true
  additionalExecutables: { initdb: bin/initdb, psql: bin/psql }
```

## connection

A list of 1 to 8 connection strings. The first is the main one.

| Field | Required | Rules |
| --- | --- | --- |
| `name` | yes | Lowercase letter, then up to 31 lowercase letters, digits, or `_`; unique |
| `title` | yes | 1 to 64 characters, no control characters |
| `template` | yes | 1 to 512 bytes that render to an absolute URI. It may use `${endpoint.<n>.address}`, `${endpoint.<n>.port}`, `${endpoint.<n>.socket}`, `${endpoint.<n>.socketDir}`, `${instance.name}`, `${instance.id}`, `${provider.id}`, `${provider.revision}`, `${runtime.version}`, and `${secret.<field>}`. Values other than the address and the port are percent encoded. |

From `providers/nats/provider.yaml`:

```yaml
connection:
  - name: uri
    title: Client URL
    template: "nats://${endpoint.client.address}"
```

## openWith

A list of 1 to 8 actions that open an instance in another app. No template here may use `secret.<field>`, `settings.<field>`, or `${endpoint.<n>.host}`, so a password never reaches a URL, an argument, or an env value.

| Field | Required | Rules |
| --- | --- | --- |
| `name` | yes | Lowercase letter, then up to 31 lowercase letters, digits, or `_`; unique |
| `title` | yes | 1 to 48 characters, no control characters |
| `kind` | yes | `app`, `terminal`, `browser`, or `reveal`. Exactly the table with the same name must be set. |
| `copySecret` | no | Only for `app` and `terminal`: a secret field the app copies to the clipboard before it opens the action |
| `app` | with `kind: app` | `bundleIds` (1 to 4 bundle IDs, first installed wins) and `url` (1 to 512 bytes that render to an absolute URI) |
| `browser` | with `kind: browser` | `url`: 1 to 512 bytes that render to an `http` or `https` URL |
| `terminal` | with `kind: terminal` | `executable` (exactly one `${runtime.executable}` or `${runtime.executables.<alias>}` token), `args` (at most 32), `env` (at most 16, the same name rules as a command). Each arg or env value is at most 1024 bytes. |
| `reveal` | with `kind: reveal` | `configFile`: a name from `configFiles` that is not `sensitive` |

From `providers/postgresql/provider.yaml`:

```yaml
openWith:
  - name: tableplus
    title: TablePlus
    kind: app
    app:
      bundleIds: ["com.tinyapp.TablePlus"]
      url: "postgresql://postgres@${endpoint.client.address}/postgres"
    copySecret: superuserPassword
```

## remedies

A list of 1 to 8 repair steps that the doctor (`stackly doctor`) shows next to a failed start or a readiness problem it matches. A remedy is only text: Stackly never runs a command or removes a path because of one.

| Field | Required | Rules |
| --- | --- | --- |
| `name` | yes | Lowercase letter, then up to 31 lowercase letters, digits, or `_`; unique |
| `when` | yes | At least one condition, and every condition that is set must match |
| `evidence` | no | `pidFile`: a path under `${instance.data}` or `${instance.config}`, at most 300 characters, whose PID the doctor shows |
| `title` | yes | 1 to 80 characters |
| `steps` | yes | 1 to 6 steps |

`when` conditions:

| Field | Rules |
| --- | --- |
| `failureCodes` | 1 to 8 start failure codes, such as `crash_loop` or `process_exited` |
| `excerptContains` | 1 to 200 characters the log excerpt of the failure must contain |
| `readinessReasons` | 1 to 8 readiness reason codes |
| `readinessDetailContains` | 1 to 200 characters the readiness detail must contain |

Each step has `text` (1 to 300 characters), and may add `command` (at most 500 characters, a command the user may run) or `path` (at most 300 characters, a file the user may look at). `command`, `path`, and `pidFile` accept only `${instance.name}`, `${endpoint.<n>.port}`, `${instance.data}`, `${instance.config}`, `${endpoint.<n>.socketDir}`, `${runtime.executable}`, and `${runtime.executables.<alias>}`. No text may hold a control character.

From `providers/postgresql/provider.yaml`, shortened:

```yaml
remedies:
  - name: stale_postmaster_pid
    when:
      failureCodes: [crash_loop, process_exited]
      excerptContains: 'lock file "postmaster.pid" already exists'
    evidence:
      pidFile: "${instance.data}/postmaster.pid"
    title: "PostgreSQL found an old postmaster.pid"
    steps:
      - text: "Stackly never removes this file, because it also guards the data folder against a second server."
```

## Tokens

| Token | Value |
| --- | --- |
| `${instance.id}`, `${instance.name}`, `${instance.shortKey}` | The instance ID, name, and short key |
| `${instance.data}`, `${instance.config}`, `${instance.logs}`, `${instance.temp}`, `${instance.run}` | The instance folders; each needs its name in `managedFilesystem` |
| `${instance.secrets}` | The folder for secret files, inside `run` |
| `${runtime.root}`, `${runtime.executable}`, `${runtime.executables.<alias>}` | The runtime folder and its executables |
| `${runtime.version}`, `${runtime.line}` | The exact version and the line of the runtime |
| `${endpoint.<n>.host}`, `${endpoint.<n>.port}`, `${endpoint.<n>.address}` | A `tcp` or `udp` endpoint; the address is `host:port` |
| `${endpoint.<n>.socket}`, `${endpoint.<n>.socketDir}` | A `unix` endpoint: the socket file and its folder |
| `${settings.<field>}` | A setting value; an array setting is only used through `#each` in a template |
| `${secret.<field>}` | A secret setting, only where a row above says so |
| `${provider.id}`, `${provider.revision}` | From `metadata` |

A port is chosen when the instance starts, so an initialize step should not use a port token; `cargo xtask provider check` reports each one it finds.
