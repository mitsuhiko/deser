/// The encodings of property lists.
///
/// Deserialization detects the format automatically, the serializer
/// writes the format of its [`SerializerConfig`](crate::SerializerConfig).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Format {
    /// The XML format (`<plist version="1.0">`).
    ///
    /// This is the default when serializing.
    #[default]
    Xml,
    /// The binary format (`bplist00`).
    Binary,
    /// The OpenStep format, also known as ASCII or old-style property lists
    /// (`{ key = value; }`).
    ///
    /// Xcode project files (`.pbxproj`) and `.strings` files use it.  The
    /// format has no types besides strings, dictionaries, arrays and data.
    Ascii,
}

impl Format {
    /// Detects the format of a property list.
    ///
    /// Binary property lists start with `bplist`, XML documents with a
    /// markup declaration or an element.  Everything else is treated as
    /// OpenStep.  Byte order marks are skipped.
    ///
    /// ```
    /// use deser_plist::Format;
    ///
    /// assert_eq!(Format::detect(b"bplist00..."), Format::Binary);
    /// assert_eq!(Format::detect(b"<?xml version=\"1.0\"?>"), Format::Xml);
    /// assert_eq!(Format::detect(b"<plist><true/></plist>"), Format::Xml);
    /// assert_eq!(Format::detect(b"{ a = b; }"), Format::Ascii);
    /// assert_eq!(Format::detect(b"<0fbd7a>"), Format::Ascii);
    /// ```
    pub fn detect(input: &[u8]) -> Format {
        if input.starts_with(b"bplist") {
            return Format::Binary;
        }
        // for UTF-16 only ASCII characters matter, every other byte is
        // skipped.
        let (input, step) = if input.starts_with(b"\xfe\xff") {
            (&input[3.min(input.len())..], 2)
        } else if input.starts_with(b"\xff\xfe") {
            (&input[2..], 2)
        } else if let Some(rest) = input.strip_prefix(b"\xef\xbb\xbf") {
            (rest, 1)
        } else {
            (input, 1)
        };
        let mut chars = input.iter().step_by(step).copied();
        let Some(first) = chars.by_ref().find(|c| !c.is_ascii_whitespace()) else {
            return Format::Ascii;
        };
        if first != b'<' {
            return Format::Ascii;
        }
        // `<` starts data in OpenStep, which only holds hex digits and
        // whitespace.  All tags of XML plists have other letters.
        for c in chars {
            match c {
                b'>' => return Format::Ascii,
                c if c.is_ascii_hexdigit() || c.is_ascii_whitespace() => {}
                _ => return Format::Xml,
            }
        }
        Format::Xml
    }

    /// Returns `true` for the text formats (XML and OpenStep).
    pub fn is_text(self) -> bool {
        !matches!(self, Format::Binary)
    }
}

#[test]
fn test_detect() {
    assert_eq!(Format::detect(b""), Format::Ascii);
    assert_eq!(Format::detect(b"  \n"), Format::Ascii);
    assert_eq!(Format::detect(b"\xef\xbb\xbf<plist/>"), Format::Xml);
    assert_eq!(Format::detect(b"\xff\xfe<\0?\0"), Format::Xml);
    assert_eq!(Format::detect(b"\xfe\xff\0<\0?"), Format::Xml);
    assert_eq!(Format::detect(b"\xff\xfe{\0}\0"), Format::Ascii);
    assert_eq!(Format::detect(b"<dict/>"), Format::Xml);
    assert_eq!(Format::detect(b"<data>"), Format::Xml);
    assert_eq!(Format::detect(b"< 0f bd >"), Format::Ascii);
    assert_eq!(Format::detect(b"// !$*UTF8*$!\n{}"), Format::Ascii);
    assert_eq!(Format::detect(b"\"a\" = \"b\";"), Format::Ascii);
}
