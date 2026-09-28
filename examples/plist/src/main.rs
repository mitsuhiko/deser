//! Property lists with `deser-plist`.
//!
//! An `Info.plist` of an app bundle is read into a type with the key
//! names Apple uses, preferences with a date and data are written as
//! binary and XML property lists, and a `.strings` file and an old-style
//! OpenStep dictionary (where everything is a string) are read into maps,
//! numbers and booleans.  The format of the input is always detected.
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime};

use deser::{Deserialize, Serialize};
use deser_plist::{Format, SerializerConfig};

/// The keys of an `Info.plist` that the app cares about, the others are
/// ignored.
#[derive(Debug, PartialEq, Deserialize)]
struct Info {
    #[deser(rename = "CFBundleIdentifier")]
    identifier: String,
    #[deser(rename = "CFBundleShortVersionString")]
    version: String,
    #[deser(rename = "CFBundleVersion")]
    build: String,
    #[deser(rename = "LSMinimumSystemVersion")]
    minimum_system_version: String,
    /// a menu bar app, missing in most bundles
    #[deser(rename = "LSUIElement", default)]
    ui_element: bool,
    #[deser(rename = "CFBundleURLTypes", default)]
    url_types: Vec<UrlType>,
}

#[derive(Debug, PartialEq, Deserialize)]
struct UrlType {
    #[deser(rename = "CFBundleURLName")]
    name: String,
    #[deser(rename = "CFBundleURLSchemes")]
    schemes: Vec<String>,
}

const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.example.Notes</string>
	<key>CFBundleName</key>
	<string>Notes</string>
	<key>CFBundleShortVersionString</key>
	<string>2.4.1</string>
	<key>CFBundleVersion</key>
	<string>241</string>
	<key>LSMinimumSystemVersion</key>
	<string>13.0</string>
	<key>LSUIElement</key>
	<true/>
	<key>CFBundleURLTypes</key>
	<array>
		<dict>
			<key>CFBundleURLName</key>
			<string>com.example.Notes.open</string>
			<key>CFBundleURLSchemes</key>
			<array>
				<string>notes</string>
				<string>notes-beta</string>
			</array>
		</dict>
	</array>
	<key>NSHumanReadableCopyright</key>
	<string>Copyright 2024 Example</string>
</dict>
</plist>
"#;

/// Preferences as an app stores them in `~/Library/Preferences`.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "PascalCase")]
struct Preferences {
    /// a `<date>` (whole seconds in XML, microseconds in binary)
    last_opened: SystemTime,
    launch_count: u32,
    zoom: f64,
    recent_files: Vec<String>,
    /// `<data>`, base64 in XML
    bookmark: Vec<u8>,
    /// property lists have no null, `None` is left out
    proxy: Option<String>,
}

/// A `.strings` file: a dictionary without braces, with comments.
const STRINGS: &str = r#"/* Menu titles */
"menu.file" = "Datei";
"menu.quit" = "%@ beenden";

// shown on first launch
greeting = "Hallo \"Welt\"";
"#;

/// Defaults in the OpenStep format only have strings, arrays and
/// dictionaries.  Numbers and booleans are parsed by the type.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "PascalCase")]
struct Window {
    width: u32,
    height: u32,
    fullscreen: bool,
    tags: Vec<String>,
}

const WINDOW: &str = "{
    Width = 800;
    Height = 600;
    Fullscreen = NO;
    Tags = (work, \"side project\");
}";

fn main() {
    // XML: Apple's key names, unknown keys ignored
    let info: Info = deser_plist::from_slice(INFO_PLIST.as_bytes()).unwrap();
    println!("{:#?}\n", info);
    assert_eq!(info.identifier, "com.example.Notes");
    assert!(info.ui_element);
    assert_eq!(info.url_types[0].schemes, ["notes", "notes-beta"]);

    // binary and XML: dates and data are native types
    let prefs = Preferences {
        last_opened: SystemTime::UNIX_EPOCH + Duration::from_secs(1_714_560_000),
        launch_count: 17,
        zoom: 1.25,
        recent_files: vec!["~/Notes/Ideas.md".into(), "~/Notes/Todo.md".into()],
        bookmark: vec![0x62, 0x6f, 0x6f, 0x6b, 0x00, 0x02],
        proxy: None,
    };
    let binary = SerializerConfig::new()
        .format(Format::Binary)
        .to_vec(&prefs)
        .unwrap();
    println!(
        "binary: {} bytes, starts with {}",
        binary.len(),
        String::from_utf8_lossy(&binary[..8])
    );
    assert!(binary.starts_with(b"bplist00"));
    assert_eq!(Format::detect(&binary), Format::Binary);
    assert_eq!(
        deser_plist::from_slice::<Preferences>(&binary).unwrap(),
        prefs
    );

    let xml = deser_plist::to_string(&prefs).unwrap();
    println!("\n{}", xml);
    assert!(xml.contains("<date>2024-05-01T10:40:00Z</date>"));
    assert!(xml.contains("<data>\n\tYm9vawAC\n\t</data>"));
    assert!(!xml.contains("Proxy"));
    assert_eq!(
        deser_plist::from_slice::<Preferences>(xml.as_bytes()).unwrap(),
        prefs
    );

    // OpenStep: a `.strings` file into a map
    let strings: BTreeMap<String, String> = deser_plist::from_slice(STRINGS.as_bytes()).unwrap();
    for (key, value) in &strings {
        println!("{} = {}", key, value);
    }
    assert_eq!(strings["greeting"], "Hallo \"Welt\"");

    // OpenStep: every value is a string, the types parse them
    let window: Window = deser_plist::from_slice(WINDOW.as_bytes()).unwrap();
    println!("\n{:?}", window);
    assert_eq!(window.width, 800);
    assert!(!window.fullscreen);

    // and written back in the style of Xcode
    let ascii = SerializerConfig::new()
        .format(Format::Ascii)
        .to_string(&window)
        .unwrap();
    println!("\n{}", ascii);
    assert!(ascii.contains("Fullscreen = NO;"));
    assert_eq!(
        deser_plist::from_slice::<Window>(ascii.as_bytes()).unwrap(),
        window
    );

    // errors in the text formats have a line and column
    let err = deser_plist::from_slice::<Window>(b"{\n    Width = 800;\n    Height = tall;\n}")
        .unwrap_err();
    println!("error: {}", err);
    assert_eq!(err.line(), Some(3));
}
