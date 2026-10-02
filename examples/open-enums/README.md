# open-enums

```
cargo run -p open-enums
```

## Why

Enums need to know all their variants.  Plugins and configuration often
do not: a crate defines what a step of a pipeline is and other crates add
steps.  Open enums are traits whose implementations are the variants of
their trait objects (`Box<dyn Step>` and `Arc<dyn Step>`), wherever they
are.  They need the `open-enums` feature of deser.

## What it shows

- `pipeline` defines the trait `Step` with `#[deser::open_enum]`
  (internally tagged with `tag = "type"`, names in snake case) and two
  steps marked with `#[deser::variant]`.
- `pipeline-extras` adds two more steps in another crate, one of them
  renamed with an alias for its old name.
- Both crates provide a `register` function, the program registers the
  steps it accepts in an `OpenEnums` registry and reads the pipeline in a
  `Context` with it (`deserialize_in`).  Without the registry the steps
  cannot be read, writing does not need it.
- Unit structs (`Trim`) are the tag alone.

## What you should see

The parsed pipeline (`Debug`), the output of running it, the pipeline
written back as JSON (with `upper` instead of the alias `uppercase`), the
error without registry, a list of steps created in code and the error for
an unknown step, which lists the registered steps.

## How to read it

Start with `pipeline/src/lib.rs`, then `pipeline-extras/src/lib.rs` and
`src/main.rs`.

Related: `enums` (the representations), `protocol` (integer tags).
