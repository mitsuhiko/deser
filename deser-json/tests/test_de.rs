use deser_json::from_str;

#[test]
fn test_basic() {
    let x: Vec<u32> = from_str(r#"[1, 2, 3, 4]"#).unwrap();
    assert_eq!(x, vec![1, 2, 3, 4]);
}
