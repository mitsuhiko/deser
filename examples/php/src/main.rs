//! PHP's serialization format with `deser-php`.
//!
//! PHP applications store `serialize()`d values in databases, caches and
//! sessions.  This reads the user roles of a WordPress site into types,
//! reads a cached model object (with its class, protected and private
//! properties and an enum case), shows what happens with shared objects
//! (references are not resolved) and writes values that PHP reads back.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};
use deser_php::{Object, Reference};

/// The `wp_user_roles` option of WordPress: an array of roles with the
/// capabilities they have.
const WP_USER_ROLES: &[u8] = br#"a:2:{s:13:"administrator";a:2:{s:4:"name";s:13:"Administrator";s:12:"capabilities";a:3:{s:13:"switch_themes";b:1;s:11:"edit_themes";b:1;s:8:"level_10";b:1;}}s:10:"subscriber";a:2:{s:4:"name";s:10:"Subscriber";s:12:"capabilities";a:1:{s:4:"read";b:1;}}}"#;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Role {
    name: String,
    capabilities: BTreeMap<String, bool>,
}

/// A cached model, as `serialize(new User())` writes it:
///
/// ```php
/// namespace App\Models;
///
/// enum Status: string { case Active = 'active'; case Banned = 'banned'; }
///
/// class Model { protected $connection = 'mysql'; }
///
/// class User extends Model {
///     public $id = 7;
///     public $email = 'jane@example.com';
///     protected $roles = ['editor', 'author'];
///     private $passwordHash = "\x9f\x12\xab";
///     public $status = Status::Active;
///     public $settings = [];
///     public $lastLogin = 1714560000.5;
/// }
/// ```
///
/// Protected properties are prefixed with `\0*\0`, private ones with the
/// class (`\0App\Models\User\0`).
const CACHED_USER: &[u8] = b"O:15:\"App\\Models\\User\":8:{\
s:13:\"\0*\0connection\";s:5:\"mysql\";\
s:2:\"id\";i:7;\
s:5:\"email\";s:16:\"jane@example.com\";\
s:8:\"\0*\0roles\";a:2:{i:0;s:6:\"editor\";i:1;s:6:\"author\";}\
s:29:\"\0App\\Models\\User\0passwordHash\";s:3:\"\x9f\x12\xab\";\
s:6:\"status\";E:24:\"App\\Models\\Status:Active\";\
s:8:\"settings\";a:0:{}\
s:9:\"lastLogin\";d:1714560000.5;}";

/// The properties of the model the program cares about.  Properties match
/// fields by their name whatever their visibility, the others are
/// ignored.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[deser(rename_all = "camelCase")]
struct User {
    id: u64,
    email: String,
    roles: Vec<String>,
    /// PHP strings are bytes, this one is not valid UTF-8
    password_hash: Vec<u8>,
    status: Status,
    /// `[]` in PHP: the empty array is an empty list and an empty map
    settings: BTreeMap<String, String>,
    last_login: f64,
}

/// Enum cases are deserialized by the name of the case.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Status {
    Active,
    Banned,
}

/// Two posts with the same author object.  PHP writes the object once,
/// the second post refers back to it (`r:5;`, the fifth value).
const POSTS: &[u8] = br#"a:1:{s:5:"posts";a:2:{i:0;a:2:{s:5:"title";s:5:"Hello";s:6:"author";O:8:"stdClass":1:{s:4:"name";s:4:"Jane";}}i:1;a:2:{s:5:"title";s:5:"Again";s:6:"author";r:5;}}}"#;

#[derive(Debug, Deserialize)]
struct Posts {
    posts: Vec<Post>,
}

#[derive(Debug, Deserialize)]
struct Post {
    title: String,
    author: AuthorOrReference,
}

/// References are not resolved, they arrive as markers.
#[derive(Debug, Deserialize)]
#[deser(untagged)]
enum AuthorOrReference {
    Author { name: String },
    Reference(Reference),
}

/// Values for a PHP application to read.
#[derive(Serialize)]
struct Product {
    sku: String,
    price: f64,
    tax_rate: f64,
    stock: BTreeMap<String, u32>,
    tags: Vec<String>,
    discontinued: Option<bool>,
}

fn main() {
    // arrays with string keys are maps, structs take them too
    let roles: BTreeMap<String, Role> = deser_php::from_slice(WP_USER_ROLES).unwrap();
    for (key, role) in &roles {
        let capabilities: Vec<_> = role.capabilities.keys().map(String::as_str).collect();
        println!("{} ({}): {}", role.name, key, capabilities.join(", "));
    }
    assert!(roles["administrator"].capabilities["switch_themes"]);
    // and they go back the same way
    let bytes = deser_php::to_vec(&roles).unwrap();
    assert_eq!(
        deser_php::from_slice::<BTreeMap<String, Role>>(&bytes).unwrap(),
        roles
    );

    // objects are maps with a class, `Object` captures it
    let user: Object<User> = deser_php::from_slice(CACHED_USER).unwrap();
    println!("\n{:#?}", user);
    assert_eq!(user.class.as_deref(), Some("App\\Models\\User"));
    assert_eq!(user.value.roles, ["editor", "author"]);
    assert_eq!(user.value.password_hash, b"\x9f\x12\xab");
    assert_eq!(user.value.status, Status::Active);
    assert!(user.value.settings.is_empty());

    // written back it's an object of the class again.  The visibility of
    // the properties was not kept (they are public now) and enum cases are
    // strings without a class unless wrapped in `Object` too.
    let bytes = deser_php::to_vec(&user).unwrap();
    println!("\n{}", String::from_utf8_lossy(&bytes));
    assert!(bytes.starts_with(b"O:15:\"App\\Models\\User\":7:{s:2:\"id\";i:7;"));

    // references are markers: the number refers to the fifth value of the
    // input, which is gone once the input was deserialized
    let posts: Posts = deser_php::from_slice(POSTS).unwrap();
    println!();
    for post in &posts.posts {
        println!("{}: {:?}", post.title, post.author);
    }
    assert!(matches!(
        posts.posts[0].author,
        AuthorOrReference::Author { ref name } if name == "Jane"
    ));
    assert!(matches!(
        posts.posts[1].author,
        AuthorOrReference::Reference(reference) if reference.number() == 5
    ));

    // writing values for PHP: floats like PHP writes them, keys that are
    // the text of integers are integers
    let product = Product {
        sku: "MUG-01".into(),
        price: 19.9,
        tax_rate: 0.1,
        stock: BTreeMap::from([("42".into(), 3), ("warehouse".into(), 120)]),
        tags: vec!["kitchen".into(), "ceramic".into()],
        discontinued: None,
    };
    let bytes = deser_php::to_vec(&product).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    println!("\n{}", text);
    assert_eq!(
        text,
        r#"a:6:{s:3:"sku";s:6:"MUG-01";s:5:"price";d:19.9;s:8:"tax_rate";d:0.1;s:5:"stock";a:2:{i:42;i:3;s:9:"warehouse";i:120;}s:4:"tags";a:2:{i:0;s:7:"kitchen";i:1;s:7:"ceramic";}s:12:"discontinued";N;}"#
    );

    // errors have the offset in the input
    let err = deser_php::from_slice::<Vec<u32>>(b"a:2:{i:0;i:1;i:1;b:2;}").unwrap_err();
    println!("\nerror: {}", err);
    assert_eq!(err.offset(), Some(19));
}
