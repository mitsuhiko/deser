//! The adapters that accept values in more than one form, with JSON.
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use deser::adapters::{
    Borrowed, DefaultOnNull, DisplayFromStr, MapAsPairs, Millis, OneOrMany, PairsAsMap, PickFirst,
    Seconds,
};
use deser::{Deserialize, ErrorKind, Serialize};
use deser_json::{from_str, to_string};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

impl std::str::FromStr for Point {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Point, Self::Err> {
        let (x, y) = s.split_once(',').ok_or("missing comma")?;
        Ok(Point {
            x: x.parse().map_err(|_| "invalid x")?,
            y: y.parse().map_err(|_| "invalid y")?,
        })
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Shape<'a> {
    #[deser(as = PickFirst<(_, DisplayFromStr)>)]
    id: u64,
    #[deser(as = OneOrMany<PickFirst<(_, DisplayFromStr)>>)]
    points: Vec<Point>,
    #[deser(as = PickFirst<(Borrowed, _)>)]
    name: Cow<'a, str>,
    #[deser(as = DefaultOnNull)]
    tags: Vec<String>,
}

#[test]
fn test_forms() {
    let shape: Shape =
        from_str(r#"{"id": "7", "points": [{"x": 1, "y": 2}, "3,4"], "name": "a", "tags": null}"#)
            .unwrap();
    assert_eq!(
        shape,
        Shape {
            id: 7,
            points: vec![Point { x: 1, y: 2 }, Point { x: 3, y: 4 }],
            name: Cow::Borrowed("a"),
            tags: vec![],
        }
    );
    // borrowed atoms are delivered borrowed to the adapters
    assert!(matches!(shape.name, Cow::Borrowed(_)));
    assert_eq!(
        to_string(&shape).unwrap(),
        r#"{"id":7,"points":[{"x":1,"y":2},{"x":3,"y":4}],"name":"a","tags":[]}"#
    );

    let shape: Shape =
        from_str(r#"{"id": 7, "points": "5,6", "name": "b", "tags": ["t"]}"#).unwrap();
    assert_eq!(shape.points, [Point { x: 5, y: 6 }]);
    assert_eq!(
        to_string(&shape).unwrap(),
        r#"{"id":7,"points":{"x":5,"y":6},"name":"b","tags":["t"]}"#
    );
}

#[test]
fn test_error_locations() {
    // the error of the first adapter, at the location of the value in the
    // recording
    let err = from_str::<Shape>(
        r#"{"id": 7, "points": [{"x": 1,
            "y": true}], "name": "a", "tags": []}"#,
    )
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidType);
    assert_eq!((err.line(), err.column()), (Some(2), Some(18)));

    let err =
        from_str::<Shape>(r#"{"id": true, "points": [], "name": "a", "tags": []}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "InvalidType: unexpected bool, expected u64 at line 1 column 8"
    );
}

#[test]
fn test_pairs() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Grid {
        #[deser(as = MapAsPairs)]
        cells: BTreeMap<(u32, u32), char>,
        #[deser(as = PairsAsMap)]
        order: Vec<(String, u32)>,
    }

    let input = r#"{"cells":[[[0,1],"x"],[[2,0],"y"]],"order":{"b":1,"a":2,"b":3}}"#;
    let grid: Grid = from_str(input).unwrap();
    assert_eq!(grid.cells[&(2, 0)], 'y');
    assert_eq!(grid.order[2], ("b".into(), 3));
    assert_eq!(to_string(&grid).unwrap(), input);

    // a map keyed by tuples cannot be written as map
    let err = to_string(&grid.cells).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnsupportedType);

    let err = from_str::<Grid>(r#"{"cells":[[[0,1],"x"],[[0,1],"y"]],"order":{}}"#).unwrap_err();
    assert_eq!(
        err.to_string(),
        "DuplicateKey: duplicate key in map at line 1 column 34"
    );
}

#[test]
fn test_time_units() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Session {
        #[deser(as = Seconds)]
        created: SystemTime,
        #[deser(as = Seconds<f64>)]
        timeout: Duration,
        #[deser(as = Millis<String>)]
        latency: Duration,
    }

    let session: Session =
        from_str(r#"{"created": 1700000000.9, "timeout": 2, "latency": "250"}"#).unwrap();
    assert_eq!(
        session,
        Session {
            created: UNIX_EPOCH + Duration::from_millis(1_700_000_000_900),
            timeout: Duration::from_secs(2),
            latency: Duration::from_millis(250),
        }
    );
    assert_eq!(
        to_string(&session).unwrap(),
        r#"{"created":1700000000,"timeout":2.0,"latency":"250"}"#
    );
}
