#[cfg(feature = "macro")]
#[test]
fn basic() {
    let css: &str = scss_rust::include!("./input.scss");

    assert_eq!(css, "a{color:red}");
}
