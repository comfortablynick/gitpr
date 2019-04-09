#[cfg(test)]
use super::*;

#[test]
fn test_simple_clean() {
    let clean_status = "## master...origin/master";
    let expected = "\u{1b}[0m\u{1b}[38;5;14m(master)\n";

    let bufwtr = termcolor::BufferWriter::stdout(ColorChoice::Auto);
    let mut buf = bufwtr.buffer();
    simple_output(&mut buf, clean_status).unwrap();

    assert_eq!(str::from_utf8(buf.as_slice()).unwrap(), expected);
}

#[test]
fn test_simple_dirty() {
    let dirty_status = "## master...origin/master
 M src/main.rs
?? src/tests.rs";
    let expected = "\u{1b}[0m\u{1b}[38;5;14m(master)\u{1b}[0m\u{1b}[38;5;9m*\n";

    let bufwtr = termcolor::BufferWriter::stdout(ColorChoice::Auto);
    let mut buf = bufwtr.buffer();
    simple_output(&mut buf, dirty_status).unwrap();

    assert_eq!(str::from_utf8(buf.as_slice()).unwrap(), expected);
}
