//! Print git repo status. Handy for shell prompt.
mod logger;

use ansi_term::{
    ANSIString, ANSIStrings,
    Color::{Black, Fixed, Green, Red, Yellow},
    Style,
};
use anyhow::{format_err, Context};
use clap::{AppSettings, ArgSettings, Clap};
use duct::cmd;
use log::{debug, info};
use std::{convert::TryFrom, default::Default, env, io::Write, path::PathBuf, str};
#[allow(unused_imports)]
use writecolor::{Color, Style as Style2, WriteStyle};

/// `anyhow::Result` with default type of `()`
type Result<T = ()> = anyhow::Result<T>;

/// Help message for format string token
const FORMAT_STRING_USAGE: &str = "\
Tokenized string may contain:
------------------------------
%g  branch glyph ()
%n  VC name
%b  branch
%r  upstream
%a  commits ahead/behind remote
%c  current commit hash
%m  unstaged changes (modified/added/removed)
%s  staged changes (modified/added/removed)
%u  untracked files
%U  unmerged files (merge in progress)
%d  diff lines, ex: \"+20/-10\"
%t  stashed files indicator
------------------------------
";

/// Color styling for elements of prompt
#[derive(Debug, Default)]
struct Styles {
    plain:             Style,
    ahead_behind:      Style,
    branch:            Style,
    branch_glyph:      Style,
    commit:            Style,
    diff:              Style,
    dirty:             Style,
    modified_unstaged: Style,
    modified_staged:   Style,
    stash:             Style,
    untracked:         Style,
    unmerged:          Style,
    upstream:          Style,
}

impl Styles {
    /// Blue ANSI color (intense)
    const BLUE: u8 = 12;
    /// Bold silver ANSI color
    const BOLD_SILVER: u8 = 188;
    /// Cyan ANSI color (intense)
    const CYAN: u8 = 14;
    /// Gray ANSI color
    const GRAY: u8 = 245;

    /// Full format
    fn standard() -> Self {
        Styles {
            branch: Fixed(Self::BLUE).into(),
            commit: Black.on(Green),
            diff: Fixed(Self::BOLD_SILVER).into(),
            modified_unstaged: Red.into(),
            modified_staged: Red.into(),
            stash: Yellow.into(),
            untracked: Fixed(Self::GRAY).into(),
            unmerged: Red.into(),
            ..Default::default()
        }
    }

    // /// Simple git prompt emulation
    // fn simple() -> Self {
    //     Styles {
    //         branch: Fixed(Self::CYAN).into(),
    //         dirty: Red.into(),
    //         ..Default::default()
    //     }
    // }
}

/// Options from format string
#[derive(Debug, Default)]
struct Opt {
    show_ahead_behind:      bool,
    show_branch:            bool,
    show_branch_glyph:      bool,
    show_commit:            bool,
    show_diff:              bool,
    show_upstream:          bool,
    show_stashed:           bool,
    show_staged_modified:   bool,
    show_unstaged_modified: bool,
    show_untracked:         bool,
    show_unmerged:          bool,
    show_vcs:               bool,
}

/// Command line configuration
#[derive(Clap, Debug)]
#[clap(author, about, version, setting = AppSettings::ColoredHelp)]
struct Arg {
    /// Debug verbosity (ex: -v, -vv, -vvv)
    #[clap(short, long, parse(from_occurrences))]
    verbose: u8,

    /// Silence debug log output
    #[clap(short, long)]
    quiet: bool,

    /// Show indicators instead of numeric values.
    ///
    /// Does not apply to '%d' (diff), which always uses numeric values
    #[clap(short, long)]
    indicators_only: bool,

    /// Disable color in output
    #[clap(short, long)]
    no_color: bool,

    /// Skip trimming extra whitespace inside rendered format string
    ///
    /// Does not apply to `-s/--simple`. Extra space may be present if an item
    /// is in the format string but not in git repo, e.g., %t for stashed files
    #[clap(short = "t", long)]
    no_trim: bool,

    /// Simple mode (similar to factory git prompt)
    ///
    /// Does not accept format string (-f, --format)
    #[clap(short, long = "simple")]
    simple_mode: bool,

    /// Format print-f style string
    #[clap(
        short,
        long,
        value_name = "F-STRING",
        default_value = "%g %b@%c %a %m %d %s %u %t %U",
        long_about = FORMAT_STRING_USAGE
    )]
    format: String,

    /// Directory to check for status, if not current dir
    #[clap(short, long, value_name = "PATH", env = "PWD", setting = ArgSettings::HideEnvValues)]
    dir: PathBuf,
}

/// Hold status of git repo attributes
#[derive(Debug, Default)]
struct Repo {
    branch:     Option<String>,
    commit:     Option<String>,
    tag:        Option<String>,
    remote:     Option<String>,
    upstream:   Option<String>,
    stashed:    u32,
    ahead:      u32,
    behind:     u32,
    untracked:  u32,
    unmerged:   u32,
    insertions: u32,
    deletions:  u32,
    unstaged:   GitArea,
    staged:     GitArea,
}

/// Hold status of specific git area (staged, unstaged)
#[derive(Debug, Default)]
struct GitArea {
    modified: u32,
    added:    u32,
    deleted:  u32,
    renamed:  u32,
    copied:   u32,
}

impl Repo {
    const AHEAD_GLYPH: &'static str = "⇡";
    const BEHIND_GLYPH: &'static str = "⇣";
    const BRANCH_GLYPH: &'static str = "";
    const MODIFIED_GLYPH: &'static str = "Δ";
    const STASH_GLYPH: &'static str = "$";
    const UNMERGED_GLYPH: &'static str = "‼";
    const UNTRACKED_GLYPH: &'static str = "…";

    fn git_root_dir(&mut self) -> Result<String> {
        cmd!("git", "rev-parse", "--absolute-git-dir")
            .read()
            .context("cannot get root dir of git repo")
    }

    /// Get chunk insertions/deletions
    fn git_diff_numstat(&mut self) -> Result {
        let output = cmd!("git", "diff", "--numstat").read()?;
        for line in output.lines() {
            let mut split = line.split_whitespace();
            self.insertions += split.next().unwrap_or_default().parse().unwrap_or(0);
            self.deletions += split.next().unwrap_or_default().parse().unwrap_or(0);
        }
        Ok(())
    }

    /// Parse git status by line
    fn parse_status<S: AsRef<str>>(&mut self, gs: S) {
        for line in gs.as_ref().lines() {
            let mut words = line.split_whitespace();
            while let Some(word) = words.next() {
                match word {
                    "#" => {
                        while let Some(br) = words.next() {
                            match br {
                                "branch.oid" => self.commit = words.next().map(String::from),
                                "branch.head" => self.branch = self.parse_head(words.next()),
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

    /// Parse git status output, seeking tag if needed
    fn parse_head(&self, head: Option<&str>) -> Option<String> {
        match head {
            Some(br) => match br {
                "(detached)" => Some(git_tag().unwrap_or_else(|_| String::from("unknown"))),
                _ => Some(br.to_string()),
            },
            None => None,
        }
    }

    /// Write formatted branch to buffer
    fn fmt_branch(&self, buf: &mut Vec<ANSIString>, style: &Style) {
        if let Some(s) = &self.branch {
            buf.push(style.paint(s.to_string()));
        }
    }

    /// Write branch glyph to buffer
    fn fmt_branch_glyph(&self, buf: &mut Vec<ANSIString>, style: &Style) {
        buf.push(style.paint(Repo::BRANCH_GLYPH.to_string()));
    }

    /// Write formatted commit to buffer
    fn fmt_commit(&self, buf: &mut Vec<ANSIString>, style: &Style, len: usize) {
        if let Some(commit) = &self.commit {
            let display = if commit == "(initial)" {
                "(initial)"
            } else {
                commit[..len].into()
            }
            .to_string();
            buf.push(style.paint(display));
        }
    }

    /// Write formatted ahead/behind details to buffer
    fn fmt_ahead_behind(&self, buf: &mut Vec<ANSIString>, style: &Style, indicators_only: bool) {
        if self.ahead + self.behind == 0 {
            return;
        }
        if self.ahead != 0 {
            buf.push(style.paint(Repo::AHEAD_GLYPH));
            if !indicators_only {
                buf.push(style.paint(self.ahead.to_string()));
            }
        }
        if self.behind != 0 {
            buf.push(style.paint(Repo::BEHIND_GLYPH));
            if !indicators_only {
                buf.push(style.paint(self.behind.to_string()));
            }
        }
    }

    /// Write formatted +n/-n git diff numstat details to buffer
    fn fmt_diff_numstat(
        &mut self,
        buf: &mut Vec<ANSIString>,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        if !self.unstaged.has_changed() || indicators_only {
            return Ok(());
        }
        if self.insertions == 0 && self.deletions == 0 {
            self.git_diff_numstat()?;
        }
        if self.insertions > 0 {
            buf.push(style.paint("+"));
            buf.push(style.paint(self.insertions.to_string()));
            if self.deletions > 0 {
                buf.push(style.paint("/"));
            }
        }
        if self.deletions > 0 {
            buf.push(style.paint("-"));
            buf.push(style.paint(self.deletions.to_string()));
        }
        Ok(())
    }

    /// Write formatted stash details to buffer
    fn fmt_stash(
        &mut self,
        buf: &mut Vec<ANSIString>,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        let mut git = self.git_root_dir()?;
        git.push_str("/logs/refs/stash");
        let st = std::fs::read_to_string(git)
            .unwrap_or_default()
            .lines()
            .count();
        if st > 0 {
            self.stashed = u32::try_from(st)?;
            buf.push(style.paint(Repo::STASH_GLYPH.to_string()));
            if !indicators_only {
                buf.push(style.paint(st.to_string()));
            }
        }
        Ok(())
    }

    /// Write formatted untracked indicator and/or count to buffer
    fn fmt_untracked(&mut self, buf: &mut Vec<ANSIString>, style: &Style, indicators_only: bool) {
        if self.untracked > 0 {
            buf.push(style.paint(Repo::UNTRACKED_GLYPH.to_string()));
            if !indicators_only {
                buf.push(style.paint(self.untracked.to_string()));
            }
        }
    }

    /// Write formatted unmerged files indicator and/or count to buffer
    fn fmt_unmerged(&mut self, buf: &mut Vec<ANSIString>, style: &Style, indicators_only: bool) {
        if self.unmerged > 0 {
            buf.push(style.paint(Repo::UNMERGED_GLYPH.to_string()));
            if !indicators_only {
                buf.push(style.paint(self.unmerged.to_string()));
            }
        }
    }

    /// Write formatted upstream to buffer
    fn fmt_upstream(&self, buf: &mut Vec<ANSIString>, style: &Style) {
        if let Some(r) = &self.upstream {
            buf.push(style.paint(r.clone()));
        }
    }
}

impl GitArea {
    fn parse_modified(&mut self, ln: char) {
        match ln {
            'M' => self.modified += 1,
            'T' => self.modified += 1,
            'A' => self.added += 1,
            'D' => self.deleted += 1,
            'R' => self.renamed += 1,
            'C' => self.copied += 1,
            _ => (),
        }
    }

    fn fmt_modified(&self, buf: &mut Vec<ANSIString>, style: &Style, indicators_only: bool) {
        if !self.has_changed() {
            return;
        }
        buf.push(style.paint(Repo::MODIFIED_GLYPH));
        if !indicators_only {
            buf.push(style.paint(self.change_ct().to_string()));
        }
    }

    fn has_changed(&self) -> bool {
        self.added + self.deleted + self.modified + self.copied + self.renamed != 0
    }

    fn change_ct(&self) -> u32 {
        self.added + self.deleted + self.modified + self.copied + self.renamed
    }
}

/// Query for git tag, use in simple or regular options
fn git_tag() -> Result<String> {
    cmd!("git", "describe", "--tags", "--exact-match")
        .read()
        .context("invalid git tags")
}

/// Simple output to mimic default git prompt
fn simple_output<S: AsRef<str>>(git_status: S, buf: &mut Vec<u8>) -> Result {
    let mut raw_branch = "";
    let mut dirty = false;
    for line in git_status.as_ref().lines() {
        if line.starts_with("##") {
            raw_branch = &line[3..];
        } else {
            dirty = true;
            break;
        }
    }
    let split = raw_branch.split("...").collect::<Vec<&str>>();
    let branch = match split.get(0) {
        Some(b) if b.starts_with("HEAD") => git_tag().unwrap_or_else(|_| "unknown".to_string()),
        Some(b) => b.to_string(),
        None => "unknown".to_string(),
    };
    debug!(
        "Raw: {}; Split: {:?}; Branch: {}",
        raw_branch, split, branch
    );
    Style2::from_fg(Color::Ansi256(Styles::CYAN)).write_to(buf)?;
    write!(buf, "({})", branch)?;
    if dirty {
        Style2::from_fg(Color::Red).write_to(buf)?;
        write!(buf, "*")?;
    }
    // let styles = Styles::simple();
    // buf.push(styles.branch.paint("("));
    // buf.push(styles.branch.paint(branch));
    // buf.push(styles.branch.paint(")"));
    // if dirty {
    //     buf.push(styles.dirty.paint("*"));
    // }
    Style2::default().write_to(buf)?;
    Ok(())
}

/// Print output based on parsing of --format string
fn print_output(mut ri: Repo, args: &Arg, buf: &mut Vec<ANSIString>) -> Result {
    let mut fmt_str = args.format.chars();
    let styles = Styles::standard();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match c {
                    'a' => ri.fmt_ahead_behind(buf, &styles.ahead_behind, args.indicators_only),
                    'b' => ri.fmt_branch(buf, &styles.branch),
                    'c' => ri.fmt_commit(buf, &styles.commit, 7),
                    'd' => ri.fmt_diff_numstat(buf, &styles.diff, args.indicators_only)?,
                    'g' => ri.fmt_branch_glyph(buf, &styles.branch_glyph),
                    'm' => ri.unstaged.fmt_modified(
                        buf,
                        &styles.modified_unstaged,
                        args.indicators_only,
                    ),
                    'n' => buf.push(styles.plain.paint("git")),
                    'r' => ri.fmt_upstream(buf, &styles.upstream),
                    's' => {
                        ri.staged
                            .fmt_modified(buf, &styles.modified_staged, args.indicators_only)
                    }
                    't' => ri.fmt_stash(buf, &styles.stash, args.indicators_only)?,
                    'u' => ri.fmt_untracked(buf, &styles.untracked, args.indicators_only),
                    'U' => ri.fmt_unmerged(buf, &styles.unmerged, args.indicators_only),
                    '%' => buf.push(styles.plain.paint("%")),
                    _ => unreachable!(
                        "invalid format token allowed to reach print_output: \"%{}\"",
                        c
                    ),
                }
            }
        } else {
            buf.push(styles.plain.paint(c.to_string()));
        }
    }
    Ok(())
}

/// Entry point
fn main() -> Result {
    let args = Arg::parse();
    let mut opts: Opt = Default::default();
    let mut buf = Vec::new();

    if !args.quiet {
        logger::init_logger(args.verbose);
    }

    env::set_current_dir(&args.dir)?;

    if args.simple_mode {
        let status = cmd!(
            "git",
            "status",
            "--porcelain",
            "--branch",
            "--untracked-files=no",
        )
        .read()?;
        let mut buf = Vec::with_capacity(255);
        simple_output(status, &mut buf)?;
        // print!("{}", ANSIStrings(&buf));
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        lock.write_all(&buf)?;
        return Ok(());
    }
    // TODO: use env vars for format str and glyphs
    let mut fmt_str = args.format.chars();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match c {
                    'a' => opts.show_ahead_behind = true,
                    'b' => opts.show_branch = true,
                    'c' => opts.show_commit = true,
                    'd' => opts.show_diff = true,
                    'g' => opts.show_branch_glyph = true,
                    'm' => opts.show_unstaged_modified = true,
                    'n' => opts.show_vcs = true,
                    'r' => opts.show_upstream = true,
                    's' => opts.show_staged_modified = true,
                    't' => opts.show_stashed = true,
                    'u' => opts.show_untracked = true,
                    'U' => opts.show_unmerged = true,
                    '%' => continue,
                    _ => {
                        return Err(format_err!(
                            "invalid format string token \"%{}\"\n{}",
                            c,
                            FORMAT_STRING_USAGE
                        ));
                    }
                }
            }
        }
    }

    // TODO: possibly use rev-parse first
    let mut ri = Repo::default();
    let git_status = cmd!(
        "git",
        "status",
        "--porcelain=2",
        "--branch",
        if opts.show_untracked {
            "--untracked-files=normal"
        } else {
            "--untracked-files=no"
        },
    );
    debug!("{:?}", git_status);
    ri.parse_status(git_status.read()?.as_str());

    debug!("{:#?}", &ri);
    info!("{:#?}", &args);

    print_output(ri, &args, &mut buf)?;
    let colored = if args.no_trim {
        ANSIStrings(&buf).to_string()
    } else {
        ANSIStrings(&buf)
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    print!("{}", colored.trim_end());
    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use pretty_assertions::assert_eq;
//
//     #[test]
//     fn test_simple_clean() -> Result {
//         const CLEAN: &str = "## master...origin/master";
//         let expected = "\u{1b}[38;5;14m(master)\u{1b}[0m";
//
//         let mut buf = Vec::new();
//         simple_output(CLEAN, &mut buf);
//         let result = ANSIStrings(&buf).to_string();
//         assert_eq!(result, expected);
//         Ok(())
//     }
//
//     #[test]
//     fn test_simple_dirty() -> Result {
//         const DIRTY: &str = "## master...origin/master
//  M src/main.rs
// ?? src/tests.rs";
//         let expected = "\u{1b}[38;5;14m(master)\u{1b}[31m*\u{1b}[0m";
//
//         let mut buf = Vec::new();
//         simple_output(DIRTY, &mut buf);
//         let result = ANSIStrings(&buf).to_string();
//         assert_eq!(result, expected);
//         Ok(())
//     }
// }
