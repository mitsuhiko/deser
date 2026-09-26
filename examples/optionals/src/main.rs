//! Optional values.
//!
//! Values tell deser if they are currently optional (`None` is).  With
//! `skip_serializing_optionals` a struct leaves out all such fields without
//! having to annotate every field with `skip_serializing_if`.  When deserializing, `Option` fields are optional
//! by default.  `Option<Option<T>>` distinguishes a missing value from an
//! explicit null.
//!
//! This makes it easy to write partial updates ("patches") where missing
//! means "leave alone" and null means "clear".
use deser::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[deser(skip_serializing_optionals)]
pub struct ProfilePatch {
    /// `None` leaves the name alone.
    name: Option<String>,
    /// `None` leaves it alone, `Some(None)` clears it.
    nickname: Option<Option<String>>,
    /// `None` leaves the tags alone.
    tags: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct Profile {
    name: String,
    nickname: Option<String>,
    tags: Vec<String>,
}

impl Profile {
    fn apply(&mut self, patch: ProfilePatch) {
        if let Some(name) = patch.name {
            self.name = name;
        }
        if let Some(nickname) = patch.nickname {
            self.nickname = nickname;
        }
        if let Some(tags) = patch.tags {
            self.tags = tags;
        }
    }
}

fn main() {
    // only the fields that are set are written
    let patch = ProfilePatch {
        name: Some("Jane".into()),
        ..Default::default()
    };
    let json = deser_json::to_string(&patch).unwrap();
    println!("{}", json);
    assert_eq!(json, r#"{"name":"Jane"}"#);

    // an explicit null is written for `Some(None)`
    let patch = ProfilePatch {
        nickname: Some(None),
        ..Default::default()
    };
    let json = deser_json::to_string(&patch).unwrap();
    println!("{}", json);
    assert_eq!(json, r#"{"nickname":null}"#);

    // and when reading, missing and null are different
    let mut profile = Profile {
        name: "Jane".into(),
        nickname: Some("jd".into()),
        tags: vec!["admin".into()],
    };
    let patch: ProfilePatch = deser_json::from_str(r#"{"tags": ["staff"]}"#).unwrap();
    assert_eq!(patch.nickname, None);
    profile.apply(patch);
    println!("{:?}", profile);
    assert_eq!(profile.nickname.as_deref(), Some("jd"));

    let patch: ProfilePatch = deser_json::from_str(r#"{"nickname": null}"#).unwrap();
    assert_eq!(patch.nickname, Some(None));
    profile.apply(patch);
    println!("{:?}", profile);
    assert_eq!(profile.nickname, None);
    assert_eq!(profile.tags, ["staff"]);
}
