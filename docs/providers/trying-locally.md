# Trying a provider locally

The validator and the harness ([testing.md](testing.md)) never need the app. When you want to see your package in your own Stackly, with its catalog card, create sheet, and instances, you import it. An imported package is a local provider: Stackly reads it, shows it, and runs nothing from it until you approve it.

Every command on this page talks to the Stackly daemon, so `stacklyd` must be running. `<provider>` is always the full ID with its namespace, such as `acme/mytool`.

## What can be imported

- Only a package folder: `provider.yaml`, `schemas/settings.schema.json`, `templates/`, and `versions.json`, as in [authoring.md](authoring.md#package-layout).
- Only a namespace of your own. Import refuses the `official`, `testing`, and `stackly` namespaces.
- A package whose package and authoring checks find no error. These are the checks of `stackly provider validate` with two differences: an import does not compare the folder name with the name part of `metadata.id`, and it never reads `tests/`, so `tests/scenarios.yaml` is not checked. Warnings do not block an import.
- At most 32 imported providers, 16 MiB of package files in all.

A package made with `cargo xtask provider new` is `testing/<name>`, so it cannot be imported as it is: the `testing` namespace exists only for the harness. To try it in the app, copy the folder, set `metadata.id` to your own namespace (for example `acme/mytool`), and rename the folder to the name part of that ID. A package that installs a runtime also needs artifacts with a real `https` URL and the real `sha256` of the download; the fixture artifacts of a `testing` package are refused.

A package meant for the `official` namespace is tried through a development catalog instead: build one with `cargo xtask catalog build` and start the daemon with `STACKLY_CATALOG_DIR` pointing at it.

## Import and approve

```bash
stackly provider validate <dir>
stackly provider import <dir> --dry-run
stackly provider import <dir>
stackly provider inspect <provider>
stackly provider approve <provider>
```

1. `stackly provider validate <dir>` runs the package checks of the validator, the folder name and `tests/scenarios.yaml` included, and writes nothing. The import runs the same checks again without those two and adds its own rules (namespace, artifacts, limits).
2. `stackly provider import <dir> --dry-run` previews the package: its ID, revision, publisher, digests, the permissions it asks for, and every finding. Nothing is written.
3. `stackly provider import <dir>` copies the package into Stackly's own store. The folder you imported from is never changed, and Stackly keeps its own copy, so later edits to your folder change nothing until you import again.
4. `stackly provider inspect <provider>` shows the entry. `stackly provider list` shows it with `local` in the `TRUST` column and `needs approval` as its state.
5. `stackly provider approve <provider>` shows the identity, the permissions, and the approval statement, then asks before it approves.

The approval statement, word for word:

> This provider runs native programs on your Mac with your user account’s permissions. Stackly does not sandbox them, so they can read and change the same files you can. A matching checksum only shows that a download was not altered. It does not show that the program is safe.

`stackly provider import <dir> --approve` imports and then asks for the approval in one command. Without a terminal, or with `--json`, `approve`, `revoke`, `remove`, and `import --approve` need `--yes`, and nothing changes without it.

After the approval, the provider works like any other: `stackly runtime install` installs its runtime, and `stackly instance create` creates instances from it.

In the app, the Catalog toolbar has **Import Provider…**, which opens a folder panel and the Import Provider sheet. The detail panel of a local entry has **Approve…**, **Revoke Approval…**, and **Remove…**.

## What an approval covers

- An approval covers exactly the bytes you approved: every file of the package, `versions.json` included (the `packageSha256` shown in the preview and in `inspect`).
- Any change to the package is a new package. Import it again and its approval is `pending`, whatever changed; the preview lists what differs from the version you decided on.
- Importing exactly the bytes you decided on again brings that decision back.
- A local entry is always labelled `local` ("Local, not verified"), and its publisher is always the namespace followed by "(not verified)". It is never Tested, whatever compatibility records the package carries.
- A folder downloaded from the internet carries the `com.apple.quarantine` attribute. Stackly only reads and reports it; it never adds, changes, or removes it.

## Approval states

| Approval | Install and new instances | Existing instances |
| --- | --- | --- |
| `pending` | Refused with `provider.approval_required` | Start, stop, and change as before |
| `approved` | Allowed | Start, stop, and change as before |
| `revoked` | Refused with `provider.revoked` | Start, stop, and change as before |

## Revoke and remove

```bash
stackly provider revoke <provider>
stackly provider remove <provider>
```

- `stackly provider revoke <provider>` revokes the approval, including for revisions you approved earlier. Installed runtimes, instances, and their data stay as they are, and existing instances can still start. You can approve the same bytes again later.
- `stackly provider remove <provider>` removes the entry and revokes its approval. Instances created from it, their data, and installed runtimes are not touched. To create new instances from it again, import and approve it again.
- The bundled catalog can revoke a revision too, through its `revoked` list. Such a revision is refused with `provider.revoked`, and its approval is refused. The list is the one in the catalog of the Stackly version you run; Stackly never fetches one.

Neither revoke nor remove ever deletes a runtime or data.

## Exit codes

| Code | Exit |
| --- | --- |
| `provider.approval_required`, `provider.revoked` | 4 |
| `provider.import_changed`, `provider.import_limit`, `provider.not_approved` | 3 |
| `manifest.invalid`, `provider.import_unreadable`, `cli.provider_invalid`, `cli.confirmation_required` | 2 |
| `provider.store_failed` | 7 |
| `cli.confirmation_declined` | 9 |
| `catalog.provider_not_found`, `provider.import_not_found` | 10 |

## Pull requests from others

Maintainers never import a package from a pull request into their own Stackly before the review is done. Reviewing is reading the package and running the validator and the harness in a test root ([review.md](review.md)); importing and approving it would run its programs with the reviewer's account.
