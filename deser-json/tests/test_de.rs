use deser::Deserialize;
use deser_json::from_str;

#[test]
fn test_basic() {
    let x: Vec<u32> = from_str(r#"[1, 2, 3, 4]"#).unwrap();
    assert_eq!(x, vec![1, 2, 3, 4]);
}

#[test]
fn test_flatten() {
    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct User {
        id: u64,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    pub struct Attrs {
        is_active: bool,
        is_admin: bool,
        flags: Vec<String>,
    }

    let user: User = from_str(
        r#"
        {
            "id": 42,
            "is_active": true,
            "is_admin": true,
            "flags": ["german", "staff"]
        }
    "#,
    )
    .unwrap();

    assert_eq!(
        user,
        User {
            id: 42,
            attrs: Attrs {
                is_active: true,
                is_admin: true,
                flags: vec!["german".into(), "staff".into()],
            }
        }
    )
}
