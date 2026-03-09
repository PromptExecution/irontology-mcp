fn main() {
    lalrpop::process_root().expect("generate dsl parser");
}
