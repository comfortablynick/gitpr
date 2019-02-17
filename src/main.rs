#[macro_use]
extern crate log;
extern crate env_logger;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::{env, str};

// struct Options {
//
// }

#[derive(Debug)]
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
    unstaged: GitArea,
    staged: GitArea,
}

#[derive(Debug)]
struct GitArea {
    modified: u32,
    added: u32,
    deleted: u32,
    renamed: u32,
    copied: u32,
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
            unstaged: GitArea {
                modified: 0,
                added: 0,
                deleted: 0,
                renamed: 0,
                copied: 0,
            },
            staged: GitArea {
                modified: 0,
                added: 0,
                deleted: 0,
                renamed: 0,
                copied: 0,
            },
        }
    }
    /* Parse git status by line */
    fn parse_status(&mut self, gs: &str) {
        for line in gs.lines() {
            let mut words = line.split_whitespace();
            // scan by word
            while let Some(word) = words.next() {
                if word == "#" {
                    while let Some(br) = words.next() {
                        if br == "branch.oid" {
                            self.commit = words.next().map(String::from);
                        }
                        if br == "branch.head" {
                            self.branch = words.next().map(String::from);
                        }
                        if br == "branch.upstream" {
                            self.upstream = words.next().map(String::from);
                        }
                        if br == "branch.ab" {
                            self.ahead = words.next().map_or(0, |s| s.parse().unwrap());
                            self.behind = words.next().map_or(0, |s| s[1..].parse().unwrap());
                        }
                    }
                }
                // Tracked file
                if word == "1" || word == "2" {
                    self.parse_modified(words.next().unwrap());
                }
                if word == "u" {
                    trace!("Unmerged file: {}", line);
                }
                if word == "?" {
                    trace!("Untracked file: {}", line);
                }
            }
        }
    }

    fn parse_modified(&mut self, ln: &str) -> () {
        match &ln[..1] {
            "M" => self.staged.modified += 1,
            "A" => self.staged.added += 1,
            "D" => self.staged.deleted += 1,
            "R" => self.staged.renamed += 1,
            "C" => self.staged.copied += 1,
            _ => (),
        }
        match &ln[1..] {
            "M" => self.unstaged.modified += 1,
            "A" => self.unstaged.added += 1,
            "D" => self.unstaged.deleted += 1,
            "R" => self.unstaged.renamed += 1,
            "C" => self.unstaged.copied += 1,
            _ => (),
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
            None => String::from("terminated"),
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
    const FMT_STRING: &str = "%g (%b) %a %% %m%d%u%t %s";
    trace!("format str: {}", &FMT_STRING);
    let cmd = run("git", &["status", "--porcelain=2", "--branch"]);
    let status = str::from_utf8(&cmd.stdout).unwrap();
    let mut ri = Repo::new();
    ri.parse_status(&status);

    // parse fmt string
    let mut fmt_str = FMT_STRING.chars();
    let mut out: String = String::new();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match &c {
                    ' ' => out.push(' '),
                    'a' => out.push('a'),
                    'b' => out.push_str(&ri.branch.as_ref().unwrap().to_string()),
                    'c' => trace!("show commit"),
                    'd' => trace!("show diff"),
                    'g' => trace!("show br glyph"),
                    'm' => trace!("show modified"),
                    'n' => trace!("show VCS name"),
                    's' => trace!("show stage modified"),
                    '%' => out.push('%'),
                    _ => (),
                }
            }
        } else {
            out.push(c);
        }
    }
    info!("{:#?}", ri);
    info!("Output: {}", &out);
}
