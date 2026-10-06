//! The formats under test.
use deser::de::{Recording, StreamDeserializer};
use deser::ser::StreamSerializer;
use deser::{Context, Deserialize, Error, Serialize};

use crate::Values;

/// A format under test.
pub trait Format {
    /// The configuration of the deserializer.
    type Config: Clone;
    /// The configuration of the serializer.
    type SerConfig;
    /// The stream deserializer.
    type Stream: StreamDeserializer;
    /// The stream serializer.
    type Ser: StreamSerializer;

    /// Creates the configuration of the deserializer from the flags of the
    /// fuzz input.  The flags `0` are the default configuration.
    fn config(flags: u32, context: Context) -> Self::Config;

    /// Creates the configuration of the serializer from the flags of the
    /// fuzz input, together with the configuration of a deserializer which
    /// reads what it writes.
    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config);

    /// Deserializes a complete input.
    fn from_slice<'de, T: Deserialize<'de>>(
        config: &Self::Config,
        data: &'de [u8],
    ) -> Result<T, Error>;

    /// Deserializes the values of an input like the stream deserializer
    /// does, but with the deserializer of complete inputs.
    fn values(config: &Self::Config, data: &[u8]) -> Values;

    /// Creates the stream deserializer.
    fn stream(config: &Self::Config) -> Self::Stream;

    /// Serializes a value.
    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error>;

    /// Creates the configurations of the stream serializer and of a
    /// stream deserializer which reads what it writes (see
    /// [`check_writer`](crate::check_writer)).  Its values are written like
    /// with [`serialize`](Self::serialize).
    fn writer_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        Self::ser_config(flags, context)
    }

    /// Creates the stream serializer.
    fn serializer(config: &Self::SerConfig) -> Self::Ser;

    /// Whether the values of the stream serializer are the values of the
    /// stream deserializer (they are not with CSV, which writes documents
    /// and reads records, and form data, which joins the parameters of the
    /// values).
    const STREAM_VALUES: bool = true;

    /// Checks the raw values of the format (see [`check_raw`](crate::check_raw)).
    fn check_raw(_data: &[u8]) {}
}

/// Picks one of the values with the bits of the flags at `shift`.
fn pick<T: Copy>(flags: u32, shift: u32, choices: &[T]) -> T {
    let bits = choices.len().next_power_of_two().trailing_zeros();
    let index = (flags >> shift) as usize & ((1 << bits) - 1);
    choices[index % choices.len()]
}

fn bit(flags: u32, bit: u32) -> bool {
    flags & (1 << bit) != 0
}

/// A format that holds a single value.
fn single(rv: Result<Recording, Error>) -> Values {
    Values::collect([rv])
}

macro_rules! json_dialect {
    ($name:ident, $krate:ident, non_finite_floats: $non_finite:expr, check_raw: $check_raw:expr) => {
        pub struct $name;

        impl Format for $name {
            type Config = $krate::DeserializerConfig;
            type SerConfig = $krate::SerializerConfig;
            type Stream = $krate::StreamDeserializer;
            type Ser = $krate::Serializer;

            fn config(flags: u32, context: Context) -> Self::Config {
                use $krate::Trailing;
                $krate::DeserializerConfig::builder()
                    .trailing(pick(
                        flags,
                        0,
                        &[Trailing::Strict, Trailing::Newline, Trailing::Stop],
                    ))
                    .exact_numbers(!bit(flags, 2))
                    .context(context)
                    .build()
            }

            fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
                use $krate::{Indent, InlinePolicy};
                let config = $krate::SerializerConfig::builder()
                    .indent(pick(
                        flags,
                        0,
                        &[
                            Indent::None,
                            Indent::Spaces(2),
                            Indent::Tab,
                            Indent::Spaces(0),
                        ],
                    ))
                    .compact(!bit(flags, 2))
                    .inline(pick(
                        flags,
                        3,
                        &[
                            InlinePolicy::Never,
                            InlinePolicy::LeafIfFits(10),
                            InlinePolicy::LeafIfFits(80),
                        ],
                    ))
                    // the dialects that cannot read them back write them as
                    // null
                    .non_finite_floats($non_finite && !bit(flags, 5))
                    .build();
                (
                    config,
                    $krate::DeserializerConfig::builder()
                        .context(context)
                        .build(),
                )
            }

            fn from_slice<'de, T: Deserialize<'de>>(
                config: &Self::Config,
                data: &'de [u8],
            ) -> Result<T, Error> {
                config.from_slice(data)
            }

            fn values(config: &Self::Config, data: &[u8]) -> Values {
                let mut de = $krate::Deserializer::from_slice_with_config(data, config.clone());
                Values::collect(de.iter::<Recording>())
            }

            fn stream(config: &Self::Config) -> Self::Stream {
                $krate::StreamDeserializer::with_config(config.clone())
            }

            fn writer_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
                use $krate::Trailing;
                let trailing = pick(
                    flags,
                    6,
                    &[Trailing::Newline, Trailing::Strict, Trailing::Stop],
                );
                let (mut ser, mut de) = Self::ser_config(flags, context);
                ser.set_trailing(trailing);
                de.set_trailing(trailing);
                (ser, de)
            }

            fn serializer(config: &Self::SerConfig) -> Self::Ser {
                $krate::Serializer::with_config(config.clone())
            }

            fn serialize<T: Serialize + ?Sized>(
                config: &Self::SerConfig,
                value: &T,
            ) -> Result<Vec<u8>, Error> {
                config.to_string(value).map(String::into_bytes)
            }

            fn check_raw(data: &[u8]) {
                ($check_raw)(data)
            }
        }
    };
}

json_dialect!(Json, deser_json, non_finite_floats: false,
    check_raw: crate::check_raw::<Json, deser_json::Json>);
json_dialect!(Jsonc, deser_jsonc, non_finite_floats: false,
    check_raw: crate::check_raw::<Jsonc, deser_jsonc::Jsonc>);
json_dialect!(Json5, deser_json5, non_finite_floats: true,
    check_raw: crate::check_raw::<Json5, deser_json5::Json5>);
// Hjson has no raw values
json_dialect!(Hjson, deser_hj, non_finite_floats: false, check_raw: |_: &[u8]| {});

/// Implements the methods for formats with values that follow each other
/// (with an `iter` method on the deserializer).
macro_rules! sequence_format {
    ($krate:ident) => {
        fn from_slice<'de, T: Deserialize<'de>>(
            config: &Self::Config,
            data: &'de [u8],
        ) -> Result<T, Error> {
            config.from_slice(data)
        }

        fn values(config: &Self::Config, data: &[u8]) -> Values {
            let mut de = $krate::Deserializer::from_slice_with_config(data, config.clone());
            Values::collect(de.iter::<Recording>())
        }

        fn stream(config: &Self::Config) -> Self::Stream {
            $krate::StreamDeserializer::with_config(config.clone())
        }

        fn serializer(config: &Self::SerConfig) -> Self::Ser {
            $krate::Serializer::with_config(config.clone())
        }
    };
}

/// Implements the methods for formats with a single value.
macro_rules! single_format {
    ($krate:ident) => {
        fn from_slice<'de, T: Deserialize<'de>>(
            config: &Self::Config,
            data: &'de [u8],
        ) -> Result<T, Error> {
            config.from_slice(data)
        }

        fn values(config: &Self::Config, data: &[u8]) -> Values {
            single(config.from_slice(data))
        }

        fn stream(config: &Self::Config) -> Self::Stream {
            $krate::StreamDeserializer::with_config(config.clone())
        }

        fn serializer(config: &Self::SerConfig) -> Self::Ser {
            $krate::Serializer::with_config(config.clone())
        }
    };
}

pub struct Yaml;

impl Format for Yaml {
    type Config = deser_yaml::DeserializerConfig;
    type SerConfig = deser_yaml::SerializerConfig;
    type Stream = deser_yaml::StreamDeserializer;
    type Ser = deser_yaml::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        use deser_yaml::Version;
        deser_yaml::DeserializerConfig::builder()
            .version(pick(flags, 0, &[Version::V1_2, Version::V1_1]))
            .merge_keys(!bit(flags, 1))
            .alias_limit(if bit(flags, 2) { 100 } else { 1_000_000 })
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        use deser_yaml::{FlowPolicy, Indent, MultilineStyle, NullStyle, QuoteStyle, Version};
        let version = pick(flags, 0, &[Version::V1_2, Version::V1_1]);
        let config = deser_yaml::SerializerConfig::builder()
            .compat(version)
            .indent(pick(
                flags,
                1,
                &[
                    Indent::Spaces(2),
                    Indent::Spaces(4),
                    Indent::None,
                    Indent::Spaces(1),
                ],
            ))
            .indent_sequences(!bit(flags, 3))
            .flow(pick(
                flags,
                4,
                &[
                    FlowPolicy::Never,
                    FlowPolicy::LeafIfFits(10),
                    FlowPolicy::LeafIfFits(80),
                ],
            ))
            .fold_width(pick(flags, 6, &[None, Some(1), Some(20), Some(80)]))
            .quote_style(pick(flags, 8, &[QuoteStyle::Single, QuoteStyle::Double]))
            .quote_all(bit(flags, 9))
            .multiline(pick(
                flags,
                10,
                &[MultilineStyle::Literal, MultilineStyle::Quoted],
            ))
            .null_style(pick(
                flags,
                11,
                &[NullStyle::Null, NullStyle::Tilde, NullStyle::Empty],
            ))
            .binary(!bit(flags, 13))
            .timestamp_tag(bit(flags, 14))
            .document_start(bit(flags, 15))
            .version_directive(bit(flags, 16))
            .end_documents(bit(flags, 17))
            .build();
        (
            config,
            deser_yaml::DeserializerConfig::builder()
                .version(version)
                .context(context)
                .build(),
        )
    }

    sequence_format!(deser_yaml);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}

pub struct Toml;

impl Format for Toml {
    type Config = deser_toml::DeserializerConfig;
    type SerConfig = deser_toml::SerializerConfig;
    type Stream = deser_toml::StreamDeserializer;
    type Ser = deser_toml::Serializer;

    fn config(_flags: u32, context: Context) -> Self::Config {
        deser_toml::DeserializerConfig::builder()
            .context(context)
            .build()
    }

    fn ser_config(_flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        (
            deser_toml::SerializerConfig::new(),
            Self::config(0, context),
        )
    }

    single_format!(deser_toml);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}

pub struct Ini;

impl Ini {
    fn syntax(flags: u32) -> deser_ini::Syntax {
        pick(flags, 0, &[deser_ini::Syntax::Ini, deser_ini::Syntax::Git])
    }

    fn inline_comments(flags: u32) -> deser_ini::InlineComments {
        use deser_ini::InlineComments;
        pick(
            flags,
            1,
            &[
                InlineComments::AfterWhitespace,
                InlineComments::None,
                InlineComments::Anywhere,
            ],
        )
    }

    fn continuation(flags: u32) -> deser_ini::Continuation {
        use deser_ini::Continuation;
        pick(
            flags,
            3,
            &[
                Continuation::Indented,
                Continuation::None,
                Continuation::Backslash,
            ],
        )
    }

    fn quotes(flags: u32) -> deser_ini::Quotes {
        pick(
            flags,
            5,
            &[deser_ini::Quotes::Value, deser_ini::Quotes::None],
        )
    }
}

impl Format for Ini {
    type Config = deser_ini::DeserializerConfig;
    type SerConfig = deser_ini::SerializerConfig;
    type Stream = deser_ini::StreamDeserializer;
    type Ser = deser_ini::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        deser_ini::DeserializerConfig::builder()
            .syntax(Ini::syntax(flags))
            .inline_comments(Ini::inline_comments(flags))
            .continuation(Ini::continuation(flags))
            .quotes(Ini::quotes(flags))
            .colon_delimiter(bit(flags, 6))
            .allow_no_value(bit(flags, 7))
            .lowercase_names(bit(flags, 8))
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        let config = deser_ini::SerializerConfig::builder()
            .syntax(Ini::syntax(flags))
            .inline_comments(Ini::inline_comments(flags))
            .continuation(Ini::continuation(flags))
            .quotes(Ini::quotes(flags))
            .colon_delimiter(bit(flags, 6))
            .build();
        let de = deser_ini::DeserializerConfig::builder()
            .syntax(Ini::syntax(flags))
            .inline_comments(Ini::inline_comments(flags))
            .continuation(Ini::continuation(flags))
            .quotes(Ini::quotes(flags))
            .colon_delimiter(bit(flags, 6))
            .context(context)
            .build();
        (config, de)
    }

    single_format!(deser_ini);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}

pub struct Cbor;

impl Format for Cbor {
    type Config = deser_cbor::DeserializerConfig;
    type SerConfig = deser_cbor::SerializerConfig;
    type Stream = deser_cbor::StreamDeserializer;
    type Ser = deser_cbor::Serializer;

    fn config(_flags: u32, context: Context) -> Self::Config {
        deser_cbor::DeserializerConfig::builder()
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        let config = deser_cbor::SerializerConfig::builder()
            .canonical(bit(flags, 0))
            .build();
        (config, Self::config(0, context))
    }

    sequence_format!(deser_cbor);

    fn check_raw(data: &[u8]) {
        crate::check_raw::<Self, deser_cbor::Cbor>(data);
    }

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_vec(value)
    }
}

pub struct Msgpack;

impl Format for Msgpack {
    type Config = deser_msgpack::DeserializerConfig;
    type SerConfig = deser_msgpack::SerializerConfig;
    type Stream = deser_msgpack::StreamDeserializer;
    type Ser = deser_msgpack::Serializer;

    fn config(_flags: u32, context: Context) -> Self::Config {
        deser_msgpack::DeserializerConfig::builder()
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        let config = deser_msgpack::SerializerConfig::builder()
            .canonical(bit(flags, 0))
            .build();
        (config, Self::config(0, context))
    }

    sequence_format!(deser_msgpack);

    fn check_raw(data: &[u8]) {
        crate::check_raw::<Self, deser_msgpack::Msgpack>(data);
    }

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_vec(value)
    }
}

pub struct Xml;

impl Xml {
    fn attribute_prefix(flags: u32) -> &'static str {
        pick(flags, 0, &["@", "-"])
    }

    fn text_key(flags: u32) -> &'static str {
        pick(flags, 1, &["#text", "$value"])
    }
}

impl Format for Xml {
    type Config = deser_xml::DeserializerConfig;
    type SerConfig = deser_xml::SerializerConfig;
    type Stream = deser_xml::StreamDeserializer;
    type Ser = deser_xml::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        deser_xml::DeserializerConfig::builder()
            .attribute_prefix(Xml::attribute_prefix(flags))
            .text_key(Xml::text_key(flags))
            .resolve_namespaces(bit(flags, 2))
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        use deser_xml::Indent;
        let config = deser_xml::SerializerConfig::builder()
            .attribute_prefix(Xml::attribute_prefix(flags))
            .text_key(Xml::text_key(flags))
            .root(pick(flags, 2, &["root", "a"]))
            .declaration(bit(flags, 3))
            .indent(pick(
                flags,
                4,
                &[
                    Indent::None,
                    Indent::Spaces(2),
                    Indent::Tab,
                    Indent::Spaces(0),
                ],
            ))
            .build();
        let de = deser_xml::DeserializerConfig::builder()
            .attribute_prefix(Xml::attribute_prefix(flags))
            .text_key(Xml::text_key(flags))
            .context(context)
            .build();
        (config, de)
    }

    single_format!(deser_xml);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}

pub struct Plist;

impl Format for Plist {
    type Config = deser_plist::DeserializerConfig;
    type SerConfig = deser_plist::SerializerConfig;
    type Stream = deser_plist::StreamDeserializer;
    type Ser = deser_plist::Serializer;

    fn config(_flags: u32, context: Context) -> Self::Config {
        deser_plist::DeserializerConfig::builder()
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        use deser_plist::Format;
        let config = deser_plist::SerializerConfig::builder()
            .format(pick(
                flags,
                0,
                &[Format::Xml, Format::Binary, Format::Ascii],
            ))
            .build();
        (config, Self::config(0, context))
    }

    single_format!(deser_plist);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_vec(value)
    }
}

pub struct Php;

impl Format for Php {
    type Config = deser_php::DeserializerConfig;
    type SerConfig = deser_php::SerializerConfig;
    type Stream = deser_php::StreamDeserializer;
    type Ser = deser_php::Serializer;

    fn config(_flags: u32, context: Context) -> Self::Config {
        deser_php::DeserializerConfig::builder()
            .context(context)
            .build()
    }

    fn ser_config(_flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        (deser_php::SerializerConfig::new(), Self::config(0, context))
    }

    sequence_format!(deser_php);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_vec(value)
    }
}

pub struct Pickle;

impl Format for Pickle {
    type Config = deser_pickle::DeserializerConfig;
    type SerConfig = deser_pickle::SerializerConfig;
    type Stream = deser_pickle::StreamDeserializer;
    type Ser = deser_pickle::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        let mut builder = deser_pickle::DeserializerConfig::builder().context(context);
        if bit(flags, 0) {
            builder = builder.max_shared_events(16);
        }
        builder.build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        let config = deser_pickle::SerializerConfig::builder()
            .protocol(pick(flags, 0, &[4, 0, 1, 2, 3, 5]))
            .build();
        (config, Self::config(0, context))
    }

    sequence_format!(deser_pickle);

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_vec(value)
    }
}

pub struct Csv;

impl Csv {
    fn delimiter(flags: u32) -> u8 {
        pick(flags, 0, b",;\t|")
    }

    fn quote(flags: u32) -> Option<u8> {
        pick(flags, 2, &[Some(b'"'), Some(b'\''), None])
    }

    fn escape(flags: u32) -> deser_csv::Escape {
        use deser_csv::Escape;
        pick(
            flags,
            4,
            &[Escape::None, Escape::Backslash, Escape::Char(b'^')],
        )
    }

    fn terminator(flags: u32) -> deser_csv::Terminator {
        use deser_csv::Terminator;
        pick(
            flags,
            6,
            &[
                Terminator::Newline,
                Terminator::CrLf,
                Terminator::Byte(b'~'),
            ],
        )
    }

    fn nulls(flags: u32) -> deser_csv::Nulls {
        use deser_csv::Nulls;
        pick(flags, 8, &[Nulls::None, Nulls::Empty, Nulls::Text("NULL")])
    }
}

impl Format for Csv {
    type Config = deser_csv::DeserializerConfig;
    type SerConfig = deser_csv::SerializerConfig;
    type Stream = deser_csv::StreamDeserializer;
    type Ser = deser_csv::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        use deser_csv::{Headers, Trim};
        deser_csv::DeserializerConfig::builder()
            .delimiter(Csv::delimiter(flags))
            .quote(Csv::quote(flags))
            .escape(Csv::escape(flags))
            .terminator(Csv::terminator(flags))
            .nulls(Csv::nulls(flags))
            .double_quote(!bit(flags, 10))
            .comment(pick(flags, 11, &[None, Some(b'#')]))
            .headers(pick(
                flags,
                12,
                &[
                    Headers::First,
                    Headers::None,
                    Headers::Skip,
                    Headers::Given(&["a", "b", "c"]),
                ],
            ))
            .trim(pick(
                flags,
                14,
                &[Trim::None, Trim::Headers, Trim::Fields, Trim::All],
            ))
            .skip_blank_lines(!bit(flags, 16))
            .flexible(bit(flags, 17))
            .lenient_quotes(bit(flags, 18))
            .sep_line(bit(flags, 19))
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        use deser_csv::{Headers, QuoteStyle};
        let headers = !bit(flags, 12);
        let config = deser_csv::SerializerConfig::builder()
            .delimiter(Csv::delimiter(flags))
            .quote(Csv::quote(flags))
            .escape(Csv::escape(flags))
            .terminator(Csv::terminator(flags))
            .nulls(Csv::nulls(flags))
            .double_quote(!bit(flags, 10))
            .headers(headers)
            .quote_style(pick(
                flags,
                13,
                &[
                    QuoteStyle::Necessary,
                    QuoteStyle::Always,
                    QuoteStyle::NonNumeric,
                    QuoteStyle::Never,
                ],
            ))
            .escape_formulas(bit(flags, 15))
            .build();
        let de = deser_csv::DeserializerConfig::builder()
            .delimiter(Csv::delimiter(flags))
            .quote(Csv::quote(flags))
            .escape(Csv::escape(flags))
            .terminator(Csv::terminator(flags))
            .nulls(Csv::nulls(flags))
            .double_quote(!bit(flags, 10))
            .headers(if headers {
                Headers::First
            } else {
                Headers::None
            })
            .context(context)
            .build();
        (config, de)
    }

    fn from_slice<'de, T: Deserialize<'de>>(
        config: &Self::Config,
        data: &'de [u8],
    ) -> Result<T, Error> {
        config.from_slice(data)
    }

    fn values(config: &Self::Config, data: &[u8]) -> Values {
        let mut de = deser_csv::Deserializer::from_slice_with_config(data, config.clone());
        Values::collect(de.records::<Recording>())
    }

    fn stream(config: &Self::Config) -> Self::Stream {
        deser_csv::StreamDeserializer::with_config(config.clone())
    }

    fn serializer(config: &Self::SerConfig) -> Self::Ser {
        deser_csv::Serializer::document(config.clone())
    }

    const STREAM_VALUES: bool = false;

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}

pub struct Urlencoded;

impl Urlencoded {
    fn nesting(flags: u32) -> deser_urlencoded::Nesting {
        use deser_urlencoded::Nesting;
        pick(flags, 0, &[Nesting::Brackets, Nesting::Flat, Nesting::Dots])
    }
}

impl Format for Urlencoded {
    type Config = deser_urlencoded::DeserializerConfig;
    type SerConfig = deser_urlencoded::SerializerConfig;
    type Stream = deser_urlencoded::StreamDeserializer;
    type Ser = deser_urlencoded::Serializer;

    fn config(flags: u32, context: Context) -> Self::Config {
        deser_urlencoded::DeserializerConfig::builder()
            .nesting(Urlencoded::nesting(flags))
            .max_depth(pick(flags, 2, &[16, 0, 1, 1000]))
            .max_params(pick(flags, 4, &[4096, 0, 1, usize::MAX]))
            .context(context)
            .build()
    }

    fn ser_config(flags: u32, context: Context) -> (Self::SerConfig, Self::Config) {
        use deser_urlencoded::{ArrayFormat, Nesting};
        // with `Nesting::Flat` sequences cannot be read back as sequences
        // (`a[]` is a key)
        let nesting = pick(flags, 0, &[Nesting::Brackets, Nesting::Dots]);
        let config = deser_urlencoded::SerializerConfig::builder()
            .nesting(nesting)
            // `ArrayFormat::Repeat` is not used as sequences with a single
            // element cannot be told apart from atoms
            .arrays(pick(
                flags,
                2,
                &[ArrayFormat::Brackets, ArrayFormat::Indices],
            ))
            .space_as_plus(bit(flags, 3))
            .build();
        // the serializer does not limit the depth of values
        let de = deser_urlencoded::DeserializerConfig::builder()
            .nesting(nesting)
            .max_depth(usize::MAX)
            .max_params(usize::MAX)
            .context(context)
            .build();
        (config, de)
    }

    single_format!(deser_urlencoded);

    // the parameters of the values are joined
    const STREAM_VALUES: bool = false;

    fn serialize<T: Serialize + ?Sized>(
        config: &Self::SerConfig,
        value: &T,
    ) -> Result<Vec<u8>, Error> {
        config.to_string(value).map(String::into_bytes)
    }
}
