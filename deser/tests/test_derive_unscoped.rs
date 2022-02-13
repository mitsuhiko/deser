#[test]
fn test_unscoped() {
    #[derive(deser::Serialize, deser::Deserialize)]
    #[deser(skip_serializing_optionals)]
    pub struct Root {
        flag: Option<bool>,
        #[deser(flatten)]
        attrs: Attrs,
    }

    #[derive(deser::Serialize, deser::Deserialize)]
    #[deser(skip_serializing_optionals)]
    pub struct Attrs {
        is_active: bool,
        #[deser(skip_serializing_if = "is_false")]
        is_stuff: bool,
    }

    fn is_false(value: &bool) -> bool {
        !*value
    }
}
