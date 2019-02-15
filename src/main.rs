#[macro_use]
extern crate log;
extern crate env_logger;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::{env, str};

#[derive(Debug)]
#[allow(dead_code)]
struct Repo {
    working_dir: Option<PathBuf>,
    git_dir: Option<String>,
    branch: Option<String>,
    commit: Option<String>,
    remote: Option<String>,
    upstream: Option<String>,
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

impl Repo {
    fn new() -> Repo {
        Repo {
            working_dir: Some(env::current_dir().unwrap()),
            git_dir: None,
            branch: None,
            commit: None,
            remote: None,
            upstream: None,
            stashed: 0,
            ahead: 0,
            behind: 0,
            untracked: 0,
            unmerged: 0,
            insertions: 0,
            deletions: 0,
        }
    }
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

fn parse_branch(gs: &str) -> Repo {
    let mut ri = Repo::new();
    for line in gs.lines() {
        let mut words = line.split_whitespace();
        while let Some(word) = words.next() {
            if word.contains("branch.oid") {
                ri.branch = words.next().map(String::from);
                println!("{}", line);
            }
        }
    }
    return ri;
}

fn main() {
    std::env::set_var("RUST_LOG", "trace");
    env_logger::init();
    let cmd = run("git", &["status", "--porcelain=2", "--branch"]);
    let status = str::from_utf8(&cmd.stdout).unwrap();
    let ri = parse_branch(status);
    println!("{:#?}", ri);
}
