# deser-debug

Utility crate for deser to format serializables like `std::fmt::Debug`
would.  The Rust shape of values (struct and variant names, `Option`,
tuples, ...) is taken from their description, see `deser::ser::Describe`.
