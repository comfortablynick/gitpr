#[cfg(test)]
use super::*;

#[test]
fn test_simple_clean() {
    let clean_status = "## master...origin/master";

    assert_eq!(
        simple_output(clean_status).unwrap(),
        format!(
            "{}{}{}",
            "(".bright_cyan(),
            "master".bright_cyan(),
            ")".bright_cyan()
        )
    );
}

#[test]
fn test_simple_dirty() {
    let dirty_status = "## master...origin/master
 M src/main.rs
?? src/tests.rs";

    assert_eq!(
        simple_output(dirty_status).unwrap(),
        format!(
            "{}{}{}{}",
            "(".bright_cyan(),
            "master".bright_cyan(),
            ")".bright_cyan(),
            "*".bright_red(),
        )
    );
}
