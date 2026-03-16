use std::io::{self, Read};

use serde_json::{json, Value};

fn main() {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).expect("read stdin");
    let request: Value = serde_json::from_str(input.trim()).expect("parse request");

    let task = request["params"]["arguments"]["task"]
        .as_str()
        .unwrap_or("unknown task");
    let target = request["params"]["arguments"]["target"]
        .as_str()
        .unwrap_or("stdio://child:unknown");

    println!(
        "{}",
        json!({
            "jsonrpc": "2.0",
            "id": "forward-1",
            "result": {
                "target": target,
                "output": {
                    "answer": format!("handled: {task}")
                },
                "trace": ["mock-stdio"],
                "artifacts": ["artifact://stdio-1"]
            }
        })
    );
}
