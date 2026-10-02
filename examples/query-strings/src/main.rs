//! Query strings and HTML forms with `deser-urlencoded`.
//!
//! Everything in a query string is text.  What `limit=10` means is only
//! known to the type the value ends up in.  deser passes such text on as
//! lexical atoms which the types parse, and they stay lexical when values
//! are buffered.  This is why the following works, while with serde numbers
//! stop parsing as soon as a struct is flattened or the value goes through
//! an internally tagged or untagged enum (serde issue #1183):
//!
//! * a flattened struct for pagination,
//! * an internally tagged enum for filters (the tag can come last),
//! * repeated keys, `[]` and nested keys (`price[min]=10`),
//! * empty values for optional numbers and flags without values.
use deser::adapters::Flag;
use deser::{Deserialize, Serialize};
use deser_path::{Path, PathLayer};
use deser_urlencoded::{ArrayFormat, SerializerConfig};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Search {
    q: String,
    #[deser(default)]
    tags: Vec<String>,
    #[deser(flatten)]
    page: Page,
    #[deser(flatten)]
    filter: Option<Filter>,
    /// `?exact` without a value switches it on
    #[deser(as = Flag)]
    exact: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    limit: u32,
    /// `offset=` is `None`
    offset: Option<u32>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(tag = "kind", rename_all = "lowercase")]
pub enum Filter {
    Books { year: u16 },
    Games { players: u8, price: Range },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Range {
    min: u32,
    max: Option<u32>,
}

/// The form of a sign up page.
#[derive(Debug, Deserialize)]
pub struct SignUp {
    email: String,
    /// checkboxes send `on`
    newsletter: bool,
    /// `<select multiple>` sends the key once per option
    interests: Vec<Interest>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[deser(rename_all = "lowercase")]
pub enum Interest {
    Books,
    Games,
    Music,
}

fn main() {
    let search: Search = deser_urlencoded::from_str(
        "?q=board+games&tags=family&tags=strategy&limit=20&offset=\
         &price[min]=10&price[max]=50&players=4&kind=games&exact",
    )
    .unwrap();
    println!("{:#?}\n", search);
    assert_eq!(
        search,
        Search {
            q: "board games".into(),
            tags: vec!["family".into(), "strategy".into()],
            page: Page {
                limit: 20,
                offset: None,
            },
            filter: Some(Filter::Games {
                players: 4,
                price: Range {
                    min: 10,
                    max: Some(50),
                },
            }),
            exact: true,
        }
    );

    // without a `kind` there is no filter
    let search: Search = deser_urlencoded::from_str("q=rust&limit=10").unwrap();
    assert_eq!(search.filter, None);
    assert!(!search.exact);

    // serializing works the other way around
    let query = deser_urlencoded::to_string(&Search {
        q: "rust & c++".into(),
        tags: vec!["a".into(), "b".into()],
        page: Page {
            limit: 10,
            offset: Some(30),
        },
        filter: Some(Filter::Books { year: 2024 }),
        exact: false,
    })
    .unwrap();
    println!("{}", query);
    assert_eq!(
        query,
        "q=rust+%26+c%2B%2B&tags=a&tags=b&limit=10&offset=30&kind=books&year=2024&exact=false"
    );
    // sequences can be written with brackets instead (they are encoded like
    // browsers do)
    let search: Search = deser_urlencoded::from_str("q=x&tags[]=a&tags[]=b&limit=1").unwrap();
    let brackets = SerializerConfig::builder()
        .arrays(ArrayFormat::Brackets)
        .build();
    let query = brackets.to_string(&search).unwrap();
    println!("{}\n", query);
    assert_eq!(query, "q=x&tags%5B%5D=a&tags%5B%5D=b&limit=1&exact=false");

    // form data
    let form: SignUp = deser_urlencoded::from_str(
        "email=jane%40example.com&newsletter=on&interests=books&interests=music",
    )
    .unwrap();
    println!("{:#?}\n", form);
    assert!(form.newsletter);
    assert_eq!(form.interests, [Interest::Books, Interest::Music]);

    // errors point at the value, also in the flattened enum
    let err = deser_urlencoded::Deserializer::from_str("q=x&limit=10&kind=books&year=soon")
        .deserialize_with::<Search, _>(|driver| driver.push_layer(PathLayer::new()))
        .unwrap_err();
    println!("error: {}", err);
    assert_eq!(err.attachment::<Path>().unwrap().to_string(), "year");
    assert_eq!(err.offset(), Some(29));
}
