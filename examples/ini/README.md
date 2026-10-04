# ini

```
cargo run -p ini
```

## Why

INI has no specification, so every reader understands a file a little
differently: whether `;` after a value is a comment, whether quotes are
removed, whether an indented line continues the value above it.
`deser-ini` reads what is common today by default (based on a survey of
INI files on GitHub) and has presets for the dialects of Python's
`configparser` and git.  Everything in an INI file is text, so values
are parsed by the type they end up in, also in flattened structs and
tagged enums.

## What it shows

- An application config (`App`) in the default dialect:
  - keys before the first section (`name`, `greeting`)
  - a comment after a value and a quoted value that keeps its spaces
  - `reuse_port`, a key without value, which is `true` with `Flag`
  - `allowed_origins` on continuation lines, split with `Separated<'\n'>`
  - the `[database]` section picks a variant of an internally tagged enum
  - `flag` given twice, collected by a `Vec`
- Writing the config back: keys before sections, quotes and continuation
  lines where needed, repeated keys for lists.
- An error in a value with the path (`database.pool`) and its line.
- `setup.cfg` with `DeserializerConfig::python()`: the `;` in
  `rich ; python_version >= '3.10'` is not a comment, comment lines in
  continuation lines are skipped.
- `.gitconfig` with `DeserializerConfig::git()`: `[User]` is lowercased,
  `[remote "origin"]` is a nested map, values are quoted in parts.  An
  alias with quotes and `#` is added and the file is written with git's
  quoting.

## What you should see

The parsed `App`, the INI file written from it, the parsed `SetupCfg` and
`GitConfig`, the written `.gitconfig`, and:

```
error: InvalidValue: invalid value "sixteen", expected u32 at line 18 column 8 (path: database.pool)
```

## How to read it

Start with `APP_INI` and follow each line to the field it ends up in, then
compare `SETUP_CFG` and `GITCONFIG` with what the presets make of them.

Related: `config` (layered configuration from files and the environment),
`env` (the same "everything is text" idea).
