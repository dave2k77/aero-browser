use aero_core::VERSION;

#[test]
fn core_can_be_imported_without_runtime_dependencies() {
    assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
}
