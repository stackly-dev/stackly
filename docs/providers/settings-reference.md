# Settings reference

This page lists every keyword of `schemas/settings.schema.json`, the file that describes the settings of a provider. The app builds the create and edit forms from it, and the daemon checks every value against it. The file is a small, strict dialect of JSON Schema 2020-12: a keyword this page does not list is rejected (`settings_schema_keyword`). Your editor can check the file against [`settings.v1alpha1.schema.json`](../../schemas/provider/settings.v1alpha1.schema.json); `cargo xtask provider check` runs the full rules below.

The other two files of a package have their own pages: [manifest-reference.md](manifest-reference.md) for `provider.yaml` and [versions-reference.md](versions-reference.md) for `versions.json`.

## Root

| Keyword | Required | Rules |
| --- | --- | --- |
| `$schema` | yes | Exactly `https://json-schema.org/draft/2020-12/schema` |
| `title` | yes | The title of the settings form |
| `description` | no | One sentence about the settings |
| `type` | yes | Exactly `object` |
| `additionalProperties` | yes | Exactly `false` |
| `properties` | yes | The fields, at most 64. A field name is a lowercase letter followed by up to 31 letters or digits. The form shows the fields in the order of this object. |
| `required` | no | Names of fields the user must fill in |

The file is at most 128 KiB.

## Fields

Each value of `properties` is one field.

| Keyword | Required | Rules |
| --- | --- | --- |
| `type` | yes | `string`, `integer`, `boolean`, or `array` |
| `title` | yes | At most 64 characters; the form label |
| `description` | no | At most 512 characters; the help text under the field |
| `default` | see below | The value used when the user leaves the field empty. It must pass the field's own rules. |
| `enum` | no | Strings only: 1 to 64 unique values, each at most 128 characters and none starting with `-` |
| `minLength` | no | For `string`, counted in Unicode characters |
| `maxLength` | for `string` without `enum` | At most 4096 |
| `pattern` | no | For `string`: a regular expression of at most 512 characters, always matched against the whole value, so `^` and `$` are not needed |
| `minimum` | for `integer` | Between minus 2^53 and 2^53 |
| `maximum` | for `integer` | Between minus 2^53 and 2^53 |
| `items` | for `array` | The shape of one item, see [array items](#array-items) |
| `minItems` | no | For `array`, default 0 |
| `maxItems` | for `array` | At most 256 |
| `x-stackly` | see below | How Stackly treats the field, see [x-stackly](#x-stackly) |

Every field that is not a secret and not managed must be in `required` or have a `default`. A managed field is never in `required` and has no `default`. A secret field must be in `required` and may only use `title`, `description`, `minLength`, `maxLength`, and `x-stackly`.

From `providers/postgresql/schemas/settings.schema.json`:

```json
"logStatement": {
  "type": "string",
  "title": "Log statements",
  "description": "Which SQL statements are written to the instance log. Statements that set passwords are logged as typed.",
  "enum": ["none", "ddl", "mod", "all"],
  "default": "none",
  "x-stackly": { "apply": "reload", "visibility": "basic" }
}
```

## Array items

`items` of an array field describes one object, one level deep.

| Keyword | Required | Rules |
| --- | --- | --- |
| `type` | yes | Exactly `object` |
| `additionalProperties` | yes | Exactly `false` |
| `properties` | yes | At most 16 scalar fields. They take the field keywords above except `items`, `minItems`, `maxItems`, and `x-stackly`. |
| `required` | no | Every item field must be in `required` or have a `default` |

From `providers/traefik/schemas/settings.schema.json`, shortened:

```json
"routes": {
  "type": "array",
  "title": "Routes",
  "maxItems": 64,
  "default": [],
  "items": {
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "name": { "type": "string", "title": "Name", "maxLength": 63, "pattern": "[a-z][a-z0-9-]{0,62}" },
      "rule": { "type": "string", "title": "Rule", "minLength": 1, "maxLength": 1024 }
    },
    "required": ["name", "rule"]
  },
  "x-stackly": { "apply": "reload", "key": "name" }
}
```

## x-stackly

| Keyword | Rules |
| --- | --- |
| `apply` | When a change takes effect: `reload` (on a reload, without a restart), `restart` (on the next start), or `immutableAfterInit` (set once at create, never changed). Required unless the field is managed. A `reload` field may only be used by templates of config files that `lifecycle.reload.render` lists. |
| `visibility` | `basic` (default) or `advanced`; advanced fields sit in a section the user opens |
| `secret` | `true` for a secret, only on `type: string`. The value lives in the Keychain, and the registry only keeps a reference. |
| `printableOnly` | `true` only together with `secret: true`: the secret must hold no control character |
| `managed` | Stackly computes the value, and the form shows it locked. The value is a string that may use `instance.*`, `runtime.version`, `runtime.line`, `endpoint.*`, and `provider.*` tokens, but never `settings.*` or `secret.*`. |
| `key` | Only on an array field: the name of an item field whose value must be unique across items |

From `providers/postgresql/schemas/settings.schema.json`:

```json
"superuserPassword": {
  "type": "string",
  "title": "Superuser password",
  "minLength": 8,
  "maxLength": 128,
  "x-stackly": { "apply": "immutableAfterInit", "visibility": "basic", "secret": true, "printableOnly": true }
}
```

From `providers/traefik/schemas/settings.schema.json`:

```json
"dynamicDir": {
  "type": "string",
  "title": "Dynamic config folder",
  "maxLength": 4096,
  "x-stackly": { "managed": "${instance.config}/dynamic" }
}
```

## How values are checked

The daemon checks the values a user enters, never the app:

- An unknown key is `unknown_setting`; every other problem is `settings_value` with the path of the field.
- A managed field the user fills in is rejected.
- An integer must be a whole JSON number, so `5.0` and `1e3` are rejected.
- Text may not hold control characters.
- The item field named by `key` must be unique across the items of its array.
- Defaults fill in what the user left out.
