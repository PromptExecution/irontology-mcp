pub fn sample_source() -> &'static str {
    r#"
use std::fmt::Debug;

pub fn alpha() { beta(); }

fn beta() {}
"#
}
