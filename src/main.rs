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
    return Command::new(cmd)
        .args(args)
        .output()
        .expect("failed to run git status");
}

fn parse_branch(gs: &str) -> () {
    // info!("parse_branch received:\n{}", gs);
    // TODO: iterate over this instead of collecting in vector
    let s: Vec<&str> = gs.split("\n").collect();
    info!("{:#?}", s);
    // for line in gs.chars() {
    //     println!("{}", line);
    // }
}

fn main() {
    std::env::set_var("RUST_LOG", "trace");
    env_logger::init();
    let cmd = run("git", &["status", "--porcelain=2", "--branch"]);
    let status = str::from_utf8(&cmd.stdout).unwrap();
    parse_branch(status);
}
