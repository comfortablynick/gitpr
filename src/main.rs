use colored::*;
use log::{info, trace};
use std::{
    env, io,
    path::PathBuf,
    process::{self, Command, Output, Stdio},
    str,
};
use structopt::StructOpt;

mod util;
use util::AppError;

// Constants + globals
const PROG: &str = env!("CARGO_PKG_NAME");
const FORMAT_STRING_USAGE: &str = "Tokenized string may contain:
    %g  branch glyph ()
    %n  VC name
    %b  branch
    %r  remote
    %a  commits ahead/behind remote
    %c  current commit hash
    %m  unstaged changes (modified/added/removed)
    %s  staged changes (modified/added/removed)
    %u  untracked files
    %d  diff lines, ex: \"+20/-10\"
    %t  stashed files indicator";
// type AnyError = Box<dyn std::error::Error + 'static>;

/// Options from format string
#[derive(Debug, Default)]
struct Opt {
    show_ahead_behind: bool,
    show_branch: bool,
    show_branch_glyph: bool,
    show_commit: bool,
    show_diff: bool,
    show_remote: bool,
    show_stashed: bool,
    show_staged_modified: bool,
    show_unstaged_modified: bool,
    show_untracked: bool,
    show_vcs: bool,
}

#[derive(StructOpt, Debug)]
#[structopt(raw(name = "PROG"), about = "git repo status for shell prompt")]
struct Arg {
    /// Debug verbosity (ex: -v, -vv, -vvv)
    #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    verbose: u8,

    /// Show indicators instead of numeric values.
    ///
    /// Does not apply to '%d' (diff), which always uses numeric values
    #[structopt(short = "i", long = "indicators-only")]
    indicators_only: bool,

    /// Disable colored output
    #[structopt(short = "n", long = "no-color")]
    no_color: bool,

    /// Simple mode (similar to factory git prompt)
    ///
    /// Does not accept format string (-f, --format)
    #[structopt(short = "s", long = "simple")]
    simple_mode: bool,

    /// Test simple mode (similar to factory git prompt)
    ///
    /// Does not accept format string (-f, --format)
    #[structopt(short = "t", long = "test-simple")]
    test_simple_mode: bool,

    /// Format print-f style string
    #[structopt(
        short = "f",
        long = "format",
        default_value = "%g %b@%c %a %m %u %s",
        raw(long_help = "FORMAT_STRING_USAGE")
    )]
    format: String,

    /// Directory to check for status, if not current dir
    #[structopt(short = "d", long = "dir")]
    dir: Option<PathBuf>,
}

/// Hold status of git repo attributes
#[derive(Debug)]
struct Repo {
    working_dir: Option<PathBuf>,
    git_base_dir: Option<String>,
    branch: Option<String>,
    commit: Option<String>,
    tag: Option<String>,
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

/// Hold status of specific git area (staged, unstaged)
#[derive(Debug)]
struct GitArea {
    modified: u32,
    added: u32,
    deleted: u32,
    renamed: u32,
    copied: u32,
}

#[allow(dead_code)]
impl Repo {
    const BRANCH_GLYPH: char = '';
    const MODIFIED_GLYPH: char = 'Δ';
    const DIRTY_GLYPH: char = '✘';
    const CLEAN_GLYPH: char = '✔';
    const UNTRACKED_GLYPH: char = '?';
    const UNMERGED_GLYPH: char = '‼';
    const AHEAD_GLYPH: char = '↑';
    const BEHIND_GLYPH: char = '↓';
    const STASH_GLYPH: char = '$';

    fn new() -> Repo {
        Repo {
            working_dir: None,
            git_base_dir: None,
            branch: None,
            commit: None,
            tag: None,
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

    // TODO: simplify this -- does it have to be written to the Repo struct?
    fn git_root_dir(&mut self) -> Result<String, AppError> {
        if let Some(dir) = self.git_base_dir.clone() {
            return Ok(dir);
        }
        let cmd = exec(&["git", "rev-parse", "--absolute-git-dir"])?;
        let output = String::from_utf8(cmd.stdout)?;
        self.git_base_dir = Some(output.clone());
        Ok(output.trim().to_string())
    }

    fn git_diff_numstat(&mut self) {
        let cmd = exec(&["git", "diff", "--numstat"]).expect("error calling `git diff --numstat`");
        let output = String::from_utf8(cmd.stdout).unwrap_or_default();
        for line in output.lines() {
            let mut split = line.split_whitespace();
            self.insertions += split.next().unwrap_or_default().parse().unwrap_or(0);
            self.deletions += split.next().unwrap_or_default().parse().unwrap_or(0);
        }
    }

    /// Parse git status by line
    fn parse_status(&mut self, gs: &str) {
        for line in gs.lines() {
            let mut words = line.split_whitespace();
            // scan by word
            while let Some(word) = words.next() {
                match word {
                    "#" => {
                        while let Some(br) = words.next() {
                            match br {
                                "branch.oid" => self.commit = words.next().map(String::from),
                                "branch.head" => self.branch = words.next().map(String::from),
                                "branch.upstream" => self.upstream = words.next().map(String::from),
                                "branch.ab" => {
                                    self.ahead = words.next().map_or(0, |s| s.parse().unwrap());
                                    self.behind =
                                        words.next().map_or(0, |s| s[1..].parse().unwrap());
                                }
                                _ => (),
                            }
                        }
                    }
                    // Tracked file
                    "1" | "2" => {
                        let mut code = words.next().unwrap().chars();
                        self.staged.parse_modified(code.next().unwrap());
                        self.unstaged.parse_modified(code.next().unwrap());
                    }
                    "u" => self.unmerged += 1,
                    "?" => self.untracked += 1,
                    _ => (),
                }
            }
        }
    }

    fn fmt_branch(&self) -> String {
        match &self.branch {
            Some(s) => s.to_string(),
            None => String::new(),
        }
    }

    fn fmt_commit(&self, len: usize) -> String {
        match &self.commit {
            Some(s) => {
                if s == "(initial)" {
                    return s.to_string();
                }
                s[..len].to_string()
            }
            None => String::new(),
        }
    }

    fn fmt_ahead_behind(&self, indicators_only: bool) -> String {
        let mut out = String::new();
        if self.ahead != 0 {
            out.push(Repo::AHEAD_GLYPH);
            if !indicators_only {
                out.push_str(&self.ahead.to_string());
            }
        }
        if self.behind != 0 {
            out.push(Repo::BEHIND_GLYPH);
            if !indicators_only {
                out.push_str(&self.behind.to_string());
            }
        }
        out
    }

    fn fmt_diff_numstat(&mut self) -> String {
        self.git_diff_numstat();
        let mut out = String::new();
        if self.insertions > 0 {
            out.push_str("+");
            out.push_str(&self.insertions.to_string());
            if self.deletions > 0 {
                out.push_str("/");
            }
            if self.deletions > 0 {
                out.push_str(&self.deletions.to_string());
            }
        }
        out
    }

    fn fmt_stash(&mut self, indicators_only: bool) -> Option<String> {
        let mut git = self.git_root_dir().expect("error getting root dir");
        git.push_str("/logs/refs/stash");
        let st = std::fs::read_to_string(git)
            .unwrap_or_default()
            .lines()
            .count();
        if st > 0 {
            let mut out = String::with_capacity(4);
            self.stashed = st as u32;
            out.push(Repo::STASH_GLYPH);
            if !indicators_only {
                out.push_str(&st.to_string());
            }
            return Some(out);
        }
        None
    }

    fn fmt_untracked(&self, indicators_only: bool) -> Option<String> {
        if self.untracked > 0 {
            let mut out: String = String::with_capacity(4);
            out.push(Repo::UNTRACKED_GLYPH);
            if !indicators_only {
                out.push_str(&self.untracked.to_string());
            }
            return Some(out);
        }
        None
    }

    fn fmt_clean_dirty(&self, s: String) -> String {
        if self.unstaged.has_changed() {
            return s.red().to_string();
        }
        if self.staged.has_changed() {
            return s.yellow().to_string();
        }
        s.green().to_string()
    }
}

impl GitArea {
    fn parse_modified(&mut self, ln: char) {
        match ln {
            'M' => self.modified += 1,
            'A' => self.added += 1,
            'D' => self.deleted += 1,
            'R' => self.renamed += 1,
            'C' => self.copied += 1,
            _ => (),
        }
    }

    fn fmt_modified(&self, indicators_only: bool) -> String {
        let mut out: String = String::new();
        if self.has_changed() {
            out.push(Repo::MODIFIED_GLYPH);
            if !indicators_only {
                out.push_str(&self.change_ct().to_string());
            }
        }
        out
    }

    fn has_changed(&self) -> bool {
        self.added + self.deleted + self.modified + self.copied + self.renamed != 0
    }

    fn change_ct(&self) -> u32 {
        self.added + self.deleted + self.modified + self.copied + self.renamed
    }
}

/// Query for git tag, use in simple or regular options
fn git_tag() -> Result<String, AppError> {
    let cmd = exec(&["git", "describe", "--tags", "--exact-match"])?;
    let tag = str::from_utf8(&cmd.stdout)?.to_string();
    Ok(tag)
}

/// Spawn subprocess for `cmd` and access stdout/stderr
/// Fails if process output != 0
fn exec(cmd: &[&str]) -> io::Result<Output> {
    let command = Command::new(&cmd[0])
        .args(cmd.get(1..).expect("missing args in cmd"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let result = command.wait_with_output()?;

    if !result.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            str::from_utf8(&result.stderr)
                .unwrap_or("cmd returned non-zero status")
                .trim_end(),
        ));
    }
    Ok(result)
}

fn print_simple_output() -> Result<(), AppError> {
    let cmd = exec(&["git", "symbolic-ref", "--short", "HEAD"])?;
    log::debug!("git command status: {}", cmd.status);
    let branch = str::from_utf8(&cmd.stdout)?.trim_end();
    let mut br_out = String::with_capacity(12);
    br_out.push('(');
    br_out.push_str(branch);
    br_out.push(')');
    let clean = exec(&["git", "diff-index", "--quiet", "HEAD"]).is_ok();
    log::debug!("Index clean: {}", clean);
    if !clean {
        println!("{}{}", br_out.bright_cyan(), "*".bright_red());
    } else {
        println!("{}", br_out.bright_cyan());
    }
    Ok(())
}

fn print_test_simple_output() -> Result<(), AppError> {
    let status_cmd = exec(&[
        "git",
        "status",
        "--porcelain",
        "--branch",
        "--untracked-files=no",
    ])?;
    let status = str::from_utf8(&status_cmd.stdout)?;
    let mut branch = "";
    let mut dirty = false;
    for line in status.lines() {
        if line.starts_with("##") {
            branch = &line[3..];
        } else {
            dirty = true;
            break;
        }
    }
    let mut out = String::with_capacity(12);
    out.push('(');
    out.push_str(branch);
    out.push(')');
    if dirty {
        println!("{}{}", out.bright_cyan(), "*".bright_red());
    } else {
        println!("{}", out.bright_cyan());
    }
    Ok(())
}

/// Print output based on parsing of --format string
fn print_output(mut ri: Repo, args: Arg) {
    let mut fmt_str = args.format.chars();
    let mut out: String = String::with_capacity(128);
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match &c {
                    'a' => out.push_str(&ri.fmt_ahead_behind(args.indicators_only).as_str()),
                    'b' => out.push_str(ri.fmt_branch().bright_cyan().to_string().as_str()),
                    'c' => out.push_str(ri.fmt_commit(7).black().on_green().to_string().as_str()),
                    'd' => {
                        if ri.unstaged.has_changed() {
                            out.push_str(ri.fmt_diff_numstat().as_str());
                        }
                    }
                    'g' => out.push(Repo::BRANCH_GLYPH),
                    'm' => out.push_str(
                        &ri.fmt_clean_dirty(ri.unstaged.fmt_modified(args.indicators_only))
                            .as_str(),
                    ),
                    'n' => out.push_str("git"),
                    'r' => match &ri.remote {
                        Some(r) => out.push_str(r.as_str()),
                        None => (),
                    },
                    's' => out.push_str(
                        &ri.fmt_clean_dirty(ri.staged.fmt_modified(args.indicators_only))
                            .as_str(),
                    ),
                    't' => {
                        if let Some(stash) = &ri.fmt_stash(args.indicators_only) {
                            out.push_str(&stash.yellow().to_string());
                        }
                    }
                    'u' => {
                        if let Some(untracked) = &ri.fmt_untracked(args.indicators_only) {
                            out.push_str(&untracked.blue().to_string());
                        }
                    }
                    '%' => out.push('%'),
                    &c => unreachable!("print_output: invalid flag: \"%{}\"", &c),
                }
            }
        } else {
            out.push(c);
        }
    }
    log::debug!("String capacity: {}", out.capacity());
    println!("{}", out.trim_end());
}

fn main() -> Result<(), AppError> {
    let args = Arg::from_args();
    let mut opts: Opt = Default::default();

    env::set_var(
        "RUST_LOG",
        match &args.verbose {
            0 => "warning",
            1 => "info",
            2 | _ => "trace",
        },
    );

    if args.no_color {
        colored::control::set_override(false);
    };

    if let Some(d) = &args.dir {
        env::set_current_dir(d)?;
    };

    env_logger::init();

    if args.simple_mode {
        return print_simple_output();
    }

    if args.test_simple_mode {
        return print_test_simple_output();
    }

    // TODO: use env vars for format str and glyphs
    // parse fmt string
    let mut fmt_str = args.format.chars();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match &c {
                    'a' => opts.show_ahead_behind = true,
                    'b' => opts.show_branch = true,
                    'c' => opts.show_commit = true,
                    'd' => opts.show_diff = true,
                    'g' => opts.show_branch_glyph = true,
                    'm' => opts.show_unstaged_modified = true,
                    'n' => opts.show_vcs = true,
                    's' => opts.show_staged_modified = true,
                    't' => opts.show_stashed = true,
                    'u' => opts.show_untracked = true,
                    '%' => continue,
                    &c => {
                        eprintln!(
                            "Invalid format string token: '%{}'\n{}",
                            &c, FORMAT_STRING_USAGE
                        );
                        process::exit(1);
                    }
                }
            }
        }
    }

    // TODO: possibly use rev-parse first, kill 2 birds?
    let mut git_args = [
        "git",
        "status",
        "--porcelain=2",
        "--branch",
        "--untracked-files=no",
    ];
    if opts.show_untracked {
        git_args[4] = "--untracked-files=all";
    }
    log::debug!("Cmd: {:?}", git_args);
    let git_status = exec(&git_args)?;
    let mut ri = Repo::new();
    ri.parse_status(
        str::from_utf8(&git_status.stdout)?
            // .map_err(|_| io::Error::new(io::ErrorKind::Other, "Error"))?,
    );
    trace!("{:#?}", &ri);
    info!("{:#?}", &args);
    print_output(ri, args);
    Ok(())
}
