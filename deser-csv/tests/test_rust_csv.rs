//! The parser tests of the `csv-core` crate (by Andrew Gallant, MIT or
//! Unlicense), see <https://github.com/BurntSushi/rust-csv> (commit
//! 05612e87e6e92910d40d7214b70952b8434fef9b, `csv-core/src/reader.rs`).
//!
//! The cases are the same, the expectations differ where deser-csv is
//! strict by default (the lenient result is checked as well) or does not
//! support a configuration.  Every case is read from a slice and from a
//! stream in chunks of all sizes.
use std::io::Read;

use deser_csv::{DeserializerConfig, Escape, Headers, Terminator};

type Csv = Vec<Vec<String>>;

/// A reader that returns the input in chunks of a fixed size.
struct Chunked<'a> {
    input: &'a [u8],
    size: usize,
}

impl Read for Chunked<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.size.min(buf.len()).min(self.input.len());
        buf[..len].copy_from_slice(&self.input[..len]);
        self.input = &self.input[len..];
        Ok(len)
    }
}

const BASE: DeserializerConfig = DeserializerConfig::new()
    .headers(Headers::None)
    .flexible(true);

fn csv(rows: &[&[&str]]) -> Result<Csv, String> {
    Ok(rows
        .iter()
        .map(|row| row.iter().map(|field| field.to_string()).collect())
        .collect())
}

fn error(msg: &str) -> Result<Csv, String> {
    Err(msg.to_string())
}

/// Parses the input from a slice and from a stream and checks that both
/// match the expected result.
fn parses_to(name: &str, config: &DeserializerConfig, input: &str, expected: Result<Csv, String>) {
    let rv = config
        .from_str::<Csv>(input)
        .map_err(|err| err.message().to_string());
    assert_eq!(rv, expected, "{} (slice)", name);

    // miri is slow, reading streams is covered by `test_io`
    let sizes = if cfg!(miri) {
        vec![]
    } else {
        (1..=input.len().max(1)).collect()
    };
    for size in sizes {
        let mut reader = config.reader(Chunked {
            input: input.as_bytes(),
            size,
        });
        let rv = reader
            .iter::<Vec<String>>()
            .collect::<Result<Csv, _>>()
            .map_err(|err| err.message().to_string());
        assert_eq!(rv, expected, "{} (stream in chunks of {})", name, size);
    }
}

#[test]
fn test_rows() {
    let cases: &[(&str, &str, &[&[&str]])] = &[
        ("one_row_one_field", "a", &[&["a"]]),
        ("one_row_many_fields", "a,b,c", &[&["a", "b", "c"]]),
        ("one_row_trailing_comma", "a,b,", &[&["a", "b", ""]]),
        ("one_row_one_field_lf", "a\n", &[&["a"]]),
        ("one_row_many_fields_lf", "a,b,c\n", &[&["a", "b", "c"]]),
        ("one_row_trailing_comma_lf", "a,b,\n", &[&["a", "b", ""]]),
        ("one_row_one_field_crlf", "a\r\n", &[&["a"]]),
        ("one_row_many_fields_crlf", "a,b,c\r\n", &[&["a", "b", "c"]]),
        (
            "one_row_trailing_comma_crlf",
            "a,b,\r\n",
            &[&["a", "b", ""]],
        ),
        ("one_row_one_field_cr", "a\r", &[&["a"]]),
        ("one_row_many_fields_cr", "a,b,c\r", &[&["a", "b", "c"]]),
        ("one_row_trailing_comma_cr", "a,b,\r", &[&["a", "b", ""]]),
        ("many_rows_one_field", "a\nb", &[&["a"], &["b"]]),
        (
            "many_rows_many_fields",
            "a,b,c\nx,y,z",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "many_rows_trailing_comma",
            "a,b,\nx,y,",
            &[&["a", "b", ""], &["x", "y", ""]],
        ),
        ("many_rows_one_field_lf", "a\nb\n", &[&["a"], &["b"]]),
        (
            "many_rows_many_fields_lf",
            "a,b,c\nx,y,z\n",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "many_rows_trailing_comma_lf",
            "a,b,\nx,y,\n",
            &[&["a", "b", ""], &["x", "y", ""]],
        ),
        ("many_rows_one_field_crlf", "a\r\nb\r\n", &[&["a"], &["b"]]),
        (
            "many_rows_many_fields_crlf",
            "a,b,c\r\nx,y,z\r\n",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "many_rows_trailing_comma_crlf",
            "a,b,\r\nx,y,\r\n",
            &[&["a", "b", ""], &["x", "y", ""]],
        ),
        ("many_rows_one_field_cr", "a\rb\r", &[&["a"], &["b"]]),
        (
            "many_rows_many_fields_cr",
            "a,b,c\rx,y,z\r",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "many_rows_trailing_comma_cr",
            "a,b,\rx,y,\r",
            &[&["a", "b", ""], &["x", "y", ""]],
        ),
        (
            "trailing_lines_no_record",
            "\n\n\na,b,c\nx,y,z\n\n\n",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "trailing_lines_no_record_cr",
            "\r\r\ra,b,c\rx,y,z\r\r\r",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        (
            "trailing_lines_no_record_crlf",
            "\r\n\r\n\r\na,b,c\r\nx,y,z\r\n\r\n\r\n",
            &[&["a", "b", "c"], &["x", "y", "z"]],
        ),
        ("empty", "", &[]),
        ("empty_lines", "\n\n\n\n", &[]),
        (
            "empty_lines_interspersed",
            "\n\na,b\n\n\nx,y\n\n\nm,n\n",
            &[&["a", "b"], &["x", "y"], &["m", "n"]],
        ),
        ("empty_lines_crlf", "\r\n\r\n\r\n\r\n", &[]),
        (
            "empty_lines_interspersed_crlf",
            "\r\n\r\na,b\r\n\r\n\r\nx,y\r\n\r\n\r\nm,n\r\n",
            &[&["a", "b"], &["x", "y"], &["m", "n"]],
        ),
        ("empty_lines_mixed", "\r\n\n\r\n\n", &[]),
        (
            "empty_lines_interspersed_mixed",
            "\n\r\na,b\r\n\n\r\nx,y\r\n\n\r\nm,n\r\n",
            &[&["a", "b"], &["x", "y"], &["m", "n"]],
        ),
        ("empty_lines_cr", "\r\r\r\r", &[]),
        (
            "empty_lines_interspersed_cr",
            "\r\ra,b\r\r\rx,y\r\r\rm,n\r",
            &[&["a", "b"], &["x", "y"], &["m", "n"]],
        ),
        ("bom_at_start", "\u{feff}a", &[&["a"]]),
        ("bom_in_field", "a\u{feff}", &[&["a\u{feff}"]]),
        ("bom_at_field_start", "a,\u{feff}b", &[&["a", "\u{feff}b"]]),
        ("quote_empty", "\"\"", &[&[""]]),
        ("quote_lf", "\"\"\n", &[&[""]]),
        ("quote_space", "\" \"", &[&[" "]]),
        ("quote_inner_space", "\" a \"", &[&[" a "]]),
        ("extra_record_crlf_1", "foo\n1\n", &[&["foo"], &["1"]]),
        ("extra_record_crlf_2", "foo\r\n1\r\n", &[&["foo"], &["1"]]),
    ];
    for &(name, input, expected) in cases {
        parses_to(name, &BASE, input, csv(expected));
    }
}

#[test]
fn test_dialects() {
    parses_to(
        "term_weird",
        &BASE.terminator(Terminator::Byte(b'z')),
        "zza,bzc,dzz",
        csv(&[&["a", "b"], &["c", "d"]]),
    );
    parses_to(
        "ascii_delimited",
        &BASE.delimiter(0x1f).terminator(Terminator::Byte(0x1e)),
        "a\x1fb\x1ec\x1fd",
        csv(&[&["a", "b"], &["c", "d"]]),
    );
    parses_to(
        "quote_change",
        &BASE.quote(Some(b'z')),
        "zaz",
        csv(&[&["a"]]),
    );
    parses_to(
        "quote_escapes_no_double",
        &BASE.double_quote(false).lenient_quotes(true),
        r#""a""b""#,
        // csv-core keeps the second closing quote: `a"b"`
        csv(&[&["ab"]]),
    );
    parses_to(
        "quote_escapes",
        &BASE.escape(Escape::Char(b'\\')),
        r#""a\"b""#,
        csv(&[&[r#"a"b"#]]),
    );
    parses_to(
        "quote_escapes_change",
        &BASE.escape(Escape::Char(b'z')),
        r#""az"b""#,
        csv(&[&[r#"a"b"#]]),
    );
    parses_to(
        "quote_escapes_with_comma",
        &BASE.escape(Escape::Char(b'\\')).double_quote(false),
        r#""\"A,B\"""#,
        csv(&[&[r#""A,B""#]]),
    );
    parses_to(
        "quoting_disabled",
        &BASE.quote(None),
        r#""abc,foo""#,
        csv(&[&[r#""abc"#, r#"foo""#]]),
    );
    parses_to(
        "delimiter_tabs",
        &BASE.delimiter(b'\t'),
        "a\tb",
        csv(&[&["a", "b"]]),
    );
    parses_to(
        "delimiter_weird",
        &BASE.delimiter(b'z'),
        "azb",
        csv(&[&["a", "b"]]),
    );
    // csv-core allows the delimiter as quote (and the terminator as comment
    // character), deser-csv rejects conflicting special characters
    parses_to(
        "quote_delimiter",
        &BASE.quote(Some(b',')),
        ",a,,b",
        error("the quote character conflicts with another special character"),
    );
}

#[test]
fn test_quotes() {
    // csv-core takes quotes that do not follow the rules as they are,
    // deser-csv reports them unless quotes are lenient
    parses_to(
        "quote_outer_space",
        &BASE,
        "  \"a\"  ",
        error("unexpected quote in an unquoted field"),
    );
    parses_to(
        "quote_outer_space (lenient)",
        &BASE.lenient_quotes(true),
        "  \"a\"  ",
        csv(&[&["  \"a\"  "]]),
    );
    parses_to(
        "quote_no_escapes",
        &BASE,
        r#""a\"b""#,
        error("unexpected character after a quoted field"),
    );
    parses_to(
        "quote_no_escapes (lenient)",
        &BASE.lenient_quotes(true),
        r#""a\"b""#,
        // csv-core keeps the quote at the end: `a\b"`
        csv(&[&[r#"a\b"#]]),
    );
}

#[test]
fn test_comments() {
    let config = BASE.comment(Some(b'#'));
    parses_to(
        "comment_1",
        &config,
        "foo\n# hi\nbar\n",
        csv(&[&["foo"], &["bar"]]),
    );
    parses_to(
        "comment_2",
        &config,
        "foo\n # hi\nbar\n",
        csv(&[&["foo"], &[" # hi"], &["bar"]]),
    );
    parses_to(
        "comment_4",
        &config,
        "foo,b#ar,baz",
        csv(&[&["foo", "b#ar", "baz"]]),
    );
    parses_to(
        "comment_5",
        &config,
        "foo,#bar,baz",
        csv(&[&["foo", "#bar", "baz"]]),
    );
    parses_to(
        "comment_3",
        &BASE.comment(Some(b'\n')),
        "foo\n# hi\nbar\n",
        error("the comment character must be an ASCII character that is not special"),
    );
}

#[test]
fn test_stream() {
    // stream_space and stream_comma
    parses_to("stream_space", &BASE, " ", csv(&[&[" "]]));
    parses_to("stream_comma", &BASE, ",", csv(&[&["", ""]]));
}
