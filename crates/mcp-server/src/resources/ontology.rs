use serde_json::json;

pub fn list_classes() -> serde_json::Value {
    json!({
        "classes": ["CodeSymbol", "Module", "Function", "OntologyNode"]
    })
}
