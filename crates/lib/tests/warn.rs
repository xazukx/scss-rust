use macros::TestLogger;

#[macro_use]
mod macros;

#[test]
fn warn_debug() {
    let input = "@warn 2";
    let logger = TestLogger::default();
    let options = scss_rust::Options::default().logger(&logger);
    let output = scss_rust::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "");
    assert_eq!(&[] as &[String], logger.debug_messages().as_slice());
    assert_eq!(&[String::from("2")], logger.warning_messages().as_slice());
}

#[test]
fn simple_warn_with_semicolon() {
    let input = "@warn 2;";
    let logger = TestLogger::default();
    let options = scss_rust::Options::default().logger(&logger);
    let output = scss_rust::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "");
    assert_eq!(&[] as &[String], logger.debug_messages().as_slice());
    assert_eq!(&[String::from("2")], logger.warning_messages().as_slice());
}

#[test]
fn warn_while_quiet() {
    let input = "@warn 2;";
    let logger = TestLogger::default();
    let options = scss_rust::Options::default().logger(&logger).quiet(true);
    let output = scss_rust::from_string(input.to_string(), &options).expect(input);
    assert_eq!(&output, "");
    assert_eq!(&[] as &[String], logger.debug_messages().as_slice());
    assert_eq!(&[] as &[String], logger.warning_messages().as_slice());
}
