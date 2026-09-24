# deser-location

This crate provides source locations (line and column) for
[deser](https://github.com/mitsuhiko/deser).  Formats which support it publish
the span of every event they emit into the deserializer state and the
`Spanned<T>` type picks it up.
