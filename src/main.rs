#[macro_use]
extern crate log;
extern crate env_logger;
use std::process::{Command, Output};
use std::str;

#[allow(dead_code)]
struct Repo {
    working_dir: String,
    git_dir: String,
    branch: String,
    commit: String,
    remote: String,
    upstream: String,
    stashed: u32,
    ahead: u32,
    behind: u32,
    untracked: u32,
    unmerged: u32,
    insertions: u32,
    deletions: u32,
    // Unstaged   GitArea
    // Staged     GitArea
}

fn run(cmd: &str, args: &[&str]) -> Output {
    let result = Command::new(cmd)
        .args(args)
        .output()
        .expect("failed to run git status");
    trace!(
        "Cmd {}: {} {:?}",
        match result.status.code() {
            Some(code) => format!("returned {}", code),
            None => format!("terminated"),
        },
        cmd,
        args
    );
    result
}

fn parse_branch(gs: &str) -> () {
    // info!("parse_branch received:\n{}", gs);
    for line in gs.split("\n") {
        println!("{}", line);
        for word in line.split(" ") {
            println!("{}", word)
        }
    }
}

fn main() {
    std::env::set_var("RUST_LOG", "trace");
    env_logger::init();
    let cmd = run("git", &["status", "--porcelain=2", "--branch"]);
    let status = str::from_utf8(&cmd.stdout).unwrap();
    parse_branch(status);
}
