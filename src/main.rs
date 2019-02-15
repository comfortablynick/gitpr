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
            working_dir: env::current_dir().ok(),
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
    fn parse_lines(&mut self, gs: &str) {
        for line in gs.lines() {
            let mut words = line.split_whitespace();
            while let Some(word) = words.next() {
                if word.contains("branch.oid") {
                    self.commit = words.next().map(String::from);
                }
                if word.contains("branch.head") {
                    self.branch = words.next().map(String::from);
                }
                if word.contains("branch.upstream") {
                    self.upstream = words.next().map(String::from);
                }
                if word.contains("branch.ab") {
                    self.ahead = words.next().map_or(0, |s| s.parse().unwrap());
                    self.behind = words.next().map_or(0, |s| s.parse().unwrap());
                }
            }
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

fn main() {
    std::env::set_var("RUST_LOG", "trace");
    std::env::set_var("RUST_BACKTRACE", "1");
    env_logger::init();
    let cmd = run("git", &["status", "--porcelain=2", "--branch"]);
    let status = str::from_utf8(&cmd.stdout).unwrap();
    let mut ri = Repo::new();
    ri.parse_lines(status);
    println!("{:#?}", ri);
}
