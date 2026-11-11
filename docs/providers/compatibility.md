# Compatibility and deprecation

This page says how the provider manifest format changes over time, so a package you write today keeps working, and so you know what a maintainer will refuse.

## One version marker

`apiVersion` is the only version of the manifest format. Today it is `stackly/v1alpha1`. Every build of Stackly carries a table of the versions it can load, each with a status:

| Status | Meaning |
| --- | --- |
| `current` | The version for new packages |
| `deprecated` | Still loads and runs. The validator adds one `warning` (`deprecated_api_version`) that names the current version, and `cargo xtask provider check` refuses a package in `providers/` that uses it. |

A manifest whose `apiVersion` is not in the table is refused with `unsupported_api_version`, and nothing else in the file is read: no instance is created and no lifecycle step runs. The message lists the versions that build supports.

There is no second version field, such as a minimum engine version. A package that uses an addition newer than the Stackly reading it is refused by the strict loader (`unknown_field`, `unknown_primitive`, `unknown_probe`, `unknown_template_format`), and the message names the values that build knows.

## Three kinds of change

| Kind | Examples | Rule |
| --- | --- | --- |
| Additive | A new primitive, probe type, native probe, template format, token, optional field whose default keeps the old behavior, value in a closed list, capability, or issue code | Stays in `stackly/v1alpha1`. Every package that was valid stays valid, with the same digest and the same plan. |
| Tighter rule for authors | A rule that a new package must follow, such as no port token in an initialize step | Reported as an `authoring` finding, never as a load error, so a package already pinned in a user's root keeps loading and its instances keep starting |
| Breaking | Removing a field, primitive, token, or default, or changing what it means; changing the rendered files or the argv of an existing package; making the loader stricter | Needs a new `apiVersion` |

An additive change is done only when the reference pages and the generated schemas in `schemas/provider/` cover it, an unknown value is still refused when the package is validated (never when an instance starts), and the frozen corpus still passes without changing any frozen entry.

## The frozen corpus

Every revision of an official package that ships is copied into `crates/core/tests/fixtures/compat/<name>/<revision>/` by `cargo xtask provider freeze`. Next to the copy, `frozen.json` records the start argv, the start env names, and a digest of each rendered config file for fixed inputs. A test loads every entry with each new build and compares those facts, so a change that would alter an existing package fails before it merges.

- An entry is never changed or removed.
- Freezing the same revision with a different digest is refused: raise `metadata.revision` instead.
- A `testing` package is never frozen, because it is never pinned in a user's root.

## Deprecation

A field, primitive, token, or format becomes deprecated when the spec that owns the manifest format marks it so and names what replaces it. From then on:

- the validator reports each use as a `warning` that names the replacement;
- packages in `providers/` may not use it, because `cargo xtask provider check` without arguments treats a warning in a repository package as a failure;
- it keeps working in `stackly/v1alpha1` for good, and only goes away in a new `apiVersion`.

Nothing is deprecated today.

## Moving to a new apiVersion

A new `apiVersion` comes with its own decoder next to the old one, the old version marked `deprecated` in the table, and a pure conversion from the old model to the engine model. A package is never rewritten in place, and its digest does not change. A version that shipped in a stable release keeps loading while a pinned copy of a package may still use it; dropping its decoder needs a registry migration that moves every instance to a newer revision first. The frozen corpus applies to every version.

## The IPC protocol

The protocol between the daemon, the CLI, and the app has its own version and contract digest, separate from `apiVersion`. A provider package never depends on it.
