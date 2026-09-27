# renames

```
cargo run -p renames
```

## Why

Renaming keys in a file format is a breaking change. Existing files on
disk and other programs that read the same files (and haven't been
upgraded) all depend on the old names. This example walks through a
safe two-phase migration of a deployment tool's TOML config:

- keys go from `snake_case` to `kebab-case`
- `listen` becomes `bind`
- the health check's `kind`/`data` become `type`/`config`

## What it shows

- `alias_all = "snake_case"`: every field also accepts its old
  snake_case name.
- `rename(serialize = "...", deserialize = "...")` and
  `rename_all(serialize = ..., deserialize = ...)`: read one name, write
  another.
- `alias = "listen"` for a single field.
- `tag_alias` / `content_alias` for an adjacently tagged enum.
- `#[deser(skip_deserializing, default = TOOL.to_string())]`:
  `generated_by` is written for people reading the file but never read
  back. It is always the running tool.
- The two phases:
  - `phase1` reads new and old names but writes the **old** ones, so
    programs that were not upgraded keep working.
  - `phase2` writes the **new** names and still reads the old ones.
- A name and its alias count as the same key: giving both is a
  "duplicate field" error.

## What you should see

1. Phase 1 parses `OLD_FILE` and `NEW_FILE` into equivalent structs.
   `generated_by` is `"deployctl 2.0"` in both, not the `1.4` from the
   file.
2. `phase 1 writes:` TOML with `listen =`, `kind = "command"` and
   snake_case keys.
3. `phase 2 writes:` TOML with `bind =`, `type = "http"` and kebab-case
   keys.
4. Two errors: `duplicate field `bind`` and `duplicate field `type``.

## How to read it

Put `mod phase1` and `mod phase2` side by side. Only the attributes
differ. The asserts in `main` check which names are read and written in
each phase.

Related: `input-contracts`, `enums`, `protocol` (`tag_alias`).
