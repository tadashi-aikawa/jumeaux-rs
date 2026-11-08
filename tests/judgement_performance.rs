use jumeaux::{
    addons::{self, Factory, payload::*},
    models::{Addon, Response},
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug)]
struct CountedMap(Arc<AtomicUsize>);

impl minijinja::value::Object for CountedMap {
    fn enumerate(self: &Arc<Self>) -> minijinja::value::Enumerator {
        self.0.fetch_add(1, Ordering::SeqCst);
        minijinja::value::Enumerator::Str(&["id"])
    }

    fn get_value(self: &Arc<Self>, key: &minijinja::Value) -> Option<minijinja::Value> {
        (key.as_str() == Some("id")).then(|| minijinja::Value::from(1))
    }
}

#[test]
fn prepared_context_is_not_traversed_for_each_expression() {
    let traversals = Arc::new(AtomicUsize::new(0));
    let context = jumeaux::compat::values::checked_context(minijinja::context!(
        data => minijinja::Value::from_object(CountedMap(traversals.clone()))
    ))
    .unwrap();
    assert_eq!(traversals.load(Ordering::SeqCst), 1);
    let engine = jumeaux::compat::template::TemplateEngine::new();
    for _ in 0..40 {
        assert!(engine.when("data.id == 1", &context).unwrap().truthy());
    }
    assert_eq!(traversals.load(Ordering::SeqCst), 1);
}

// Opusのreview_when_costと同じ入力・計測区間。
#[test]
fn review_when_cost() {
    let mut map = serde_json::Map::new();
    for i in 0..100_000 {
        map.insert(
            format!("k{i}"),
            json!({"id": i, "name": "x", "tags": [1, 2, 3]}),
        );
    }
    let big = Value::Object(map);
    let response = || -> Response {
        serde_json::from_value(json!({
            "body": b"{}", "type": "json", "encoding": "utf-8",
            "headers": {"Content-Type": "application/json"}, "url": "http://example.test/test",
            "status_code": 200, "elapsed": 0.1, "elapsed_sec": 0.1
        }))
        .unwrap()
    };
    let reference = JudgementReference {
        name: "n".into(),
        path: "/".into(),
        qs: Default::default(),
        headers: Default::default(),
        res_one: response(),
        res_other: response(),
        dict_one: Some(big.clone()),
        dict_other: Some(big),
    };
    let payload = JudgementPayload {
        diffs_by_cognition: None,
        regard_as_same_body: false,
        regard_as_same_header: true,
    };
    for n in [1usize, 10, 40] {
        let expressions: Vec<_> = (0..n).map(|_| "False").collect();
        let addon: Addon =
            serde_json::from_value(json!({"name":"same", "config":{"when_any":expressions}}))
                .unwrap();
        let same = addons::judgement::Factory.create(&addon).unwrap();
        let start = std::time::Instant::now();
        let result = same.exec(payload.clone(), &reference).unwrap();
        println!("same when_any x{n}: {:?}", start.elapsed());
        assert!(!result.regard_as_same_body);
    }
}
