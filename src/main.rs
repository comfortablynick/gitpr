//! Print git repo status. Handy for shell prompt.
mod logger;

// use ansi_term::{ANSIString, ANSIStrings, Style};
use anyhow::{format_err, Context};
use clap::{AppSettings, ArgSettings, Clap};
use duct::cmd;
use log::{debug, info};
use std::{
    convert::TryFrom,
    default::Default,
    env,
    io::Write,
    path::{Path, PathBuf},
    str,
};
use writecolor::{Color::*, Style};

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
struct StyleSet {
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

#[allow(dead_code)]
impl StyleSet {
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
        Self {
            branch: Blue.intense(),
            commit: Black.on(Green),
            diff: Fixed(Self::BOLD_SILVER).normal(),
            modified_unstaged: Red.into(),
            modified_staged: Red.into(),
            stash: Yellow.into(),
            untracked: Fixed(Self::GRAY).into(),
            unmerged: Red.into(),
            ..StyleSet::default()
        }
    }

    /// Simple git prompt emulation
    fn simple() -> Self {
        Self {
            branch: Fixed(Self::CYAN).into(),
            dirty: Red.into(),
            ..StyleSet::default()
        }
    }
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

    /// Simple mode 2 (development)
    #[clap(short = "S", long = "simple2")]
    simple_mode2: bool,

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
    fn fmt_branch<W: Write>(&self, buf: &mut W, style: &Style) -> Result {
        if let Some(s) = &self.branch {
            write!(buf, "{}", style.paint(s))?;
        }
        Ok(())
    }

    /// Write branch glyph to buffer
    fn fmt_branch_glyph<W: Write>(&self, buf: &mut W, style: &Style) -> Result {
        write!(buf, "{}", style.paint(Repo::BRANCH_GLYPH))?;
        Ok(())
    }

    /// Write formatted commit to buffer
    fn fmt_commit<W: Write>(&self, buf: &mut W, style: &Style, len: usize) -> Result {
        if let Some(commit) = &self.commit {
            let display = if commit == "(initial)" {
                "(initial)"
            } else {
                commit[..len].into()
            };
            write!(buf, "{}", style.paint(display))?;
        }
        Ok(())
    }

    /// Write formatted ahead/behind details to buffer
    fn fmt_ahead_behind<W: Write>(
        &self,
        buf: &mut W,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        if self.ahead + self.behind == 0 {
            return Ok(());
        }
        style.write_to(buf)?;
        if self.ahead != 0 {
            buf.write_all(Repo::AHEAD_GLYPH.as_bytes())?;
            if !indicators_only {
                write!(buf, "{}", self.ahead)?;
            }
        }
        if self.behind != 0 {
            buf.write_all(Repo::BEHIND_GLYPH.as_bytes())?;
            if !indicators_only {
                write!(buf, "{}", self.behind)?;
            }
        }
        Style::reset().write_to(buf)?;
        Ok(())
    }

    /// Write formatted +n/-n git diff numstat details to buffer
    fn fmt_diff_numstat<W: Write>(
        &mut self,
        buf: &mut W,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        if !self.unstaged.has_changed() || indicators_only {
            return Ok(());
        }
        if self.insertions == 0 && self.deletions == 0 {
            self.git_diff_numstat()?;
        }
        style.write_to(buf)?;
        if self.insertions > 0 {
            write!(buf, "+{}", self.insertions)?;
            if self.deletions > 0 {
                write!(buf, "/")?;
            }
        }
        if self.deletions > 0 {
            write!(buf, "-{}", self.deletions)?;
        }
        Style::reset().write_to(buf)?;
        Ok(())
    }

    /// Write formatted stash details to buffer
    fn fmt_stash<W: Write>(&mut self, buf: &mut W, style: &Style, indicators_only: bool) -> Result {
        let mut git = self.git_root_dir()?;
        git.push_str("/logs/refs/stash");
        let st = std::fs::read_to_string(git)
            .unwrap_or_default()
            .lines()
            .count();
        if st > 0 {
            self.stashed = u32::try_from(st)?;
            style.write_to(buf)?;
            buf.write_all(Repo::STASH_GLYPH.as_bytes())?;
            if !indicators_only {
                write!(buf, "{}", self.stashed)?;
            }
            Style::reset().write_to(buf)?;
        }
        Ok(())
    }

    /// Write formatted untracked indicator and/or count to buffer
    fn fmt_untracked<W: Write>(
        &mut self,
        buf: &mut W,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        if self.untracked > 0 {
            style.write_to(buf)?;
            buf.write_all(Repo::UNTRACKED_GLYPH.as_bytes())?;
            if !indicators_only {
                write!(buf, "{}", self.untracked)?;
            }
            Style::reset().write_to(buf)?;
        }
        Ok(())
    }

    /// Write formatted unmerged files indicator and/or count to buffer
    fn fmt_unmerged<W: Write>(
        &mut self,
        buf: &mut W,
        style: &Style,
        indicators_only: bool,
    ) -> Result {
        if self.unmerged > 0 {
            style.write_to(buf)?;
            buf.write_all(Repo::UNMERGED_GLYPH.as_bytes())?;
            if !indicators_only {
                write!(buf, "{}", self.unmerged)?;
            }
            Style::reset().write_to(buf)?;
        }
        Ok(())
    }

    /// Write formatted upstream to buffer
    fn fmt_upstream<W: Write>(&self, buf: &mut W, style: &Style) -> Result {
        if let Some(r) = &self.upstream {
            write!(buf, "{}", style.paint(r))?;
        }
        Ok(())
    }
}

impl GitArea {
    /// Parse git status to determine what has been modified
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

    fn fmt_modified<W: Write>(&self, buf: &mut W, style: &Style, indicators_only: bool) -> Result {
        if !self.has_changed() {
            return Ok(());
        }
        style.write_to(buf)?;
        buf.write(Repo::MODIFIED_GLYPH.as_bytes())?;
        if !indicators_only {
            write!(buf, "{}", self.change_ct())?;
        }
        Style::reset().write_to(buf)?;
        Ok(())
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
fn simple_output<S, W>(git_status: S, buf: &mut W) -> Result
where
    S: AsRef<str>,
    W: Write,
{
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
    let styles = StyleSet::simple();
    styles.branch.write_to(buf)?;
    write!(buf, "({})", branch)?;
    if dirty {
        styles.dirty.write_to(buf)?;
        write!(buf, "*")?;
    }
    Style::reset().write_to(buf)?;
    Ok(())
}

/// Return true if we're inside the hidden .git/ directory in a repo.
fn inside_dotgit_dir(wd: &Path) -> bool {
    for path_component in wd {
        if path_component == ".git" {
            return true;
        }
    }
    false
}

/// Return the absolute path to the .git/HEAD file, which contains the name of
/// the current branch. If the current working directory isn't in a git repo, it
/// will return None.
fn find_head(dir: &Path) -> Option<PathBuf> {
    // Iterate through all the parent directories and see if $DIR/.git/HEAD is
    // a file that exists.
    //   /home/me/projects/foo/src/bar/.git/HEAD ??? -> doesn't exist
    //   /home/me/projects/foo/src/.git/HEAD ???     -> doesn't exist
    //   /home/me/projects/foo/.git/HEAD ???         -> found it!
    for d in dir.ancestors() {
        let p = d.join(".git/HEAD");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Get absolute dir of .git; should be equivalent to `git rev-parse --absolute-git-dir`
fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    find_head(dir).and_then(|f| f.parent().map(|f| f.to_path_buf()))
}

/// Return the name of the current branch. If we're in a directory that isn't
/// inside a git repo, return `None`.
fn current_branch(wd: &Path) -> Option<String> {
    if inside_dotgit_dir(wd) {
        // Print ".git" instead of the branch name.
        return Some(".git".to_owned());
    }
    // Find the path to the .git/HEAD file.
    let path_to_head = find_head(wd)?;
    // Read .git/HEAD and extract the branch name.
    std::fs::read_to_string(path_to_head)
        .map(|s| s.trim().trim_start_matches("ref: refs/heads/").to_owned())
        .ok()
}

/// Simple output using different means
fn simple_output2(buf: &mut impl Write) -> Result {
    let _ = buf;
    let cwd = env::current_dir()?;
    let dirty = cmd!("git", "status", "--short")
        .stdout_capture()
        .run()
        .map(|out| out.stdout.len() != 0)
        .unwrap_or(false);
    if dirty {
        debug!("Repo is dirty!");
    }
    if let Some(branch) = current_branch(&cwd) {
        debug!("Current branch: {}", branch);
    }
    debug!("Absolute git dir: {:?}", find_git_dir(&cwd));
    Ok(())
}

/// Print output based on parsing of --format string
fn print_output<W: Write>(mut ri: Repo, args: &Arg, buf: &mut W) -> Result {
    let mut fmt_str = args.format.chars();
    let styles = StyleSet::standard();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match c {
                    'a' => ri.fmt_ahead_behind(buf, &styles.ahead_behind, args.indicators_only)?,
                    'b' => ri.fmt_branch(buf, &styles.branch)?,
                    'c' => ri.fmt_commit(buf, &styles.commit, 7)?,
                    'd' => ri.fmt_diff_numstat(buf, &styles.diff, args.indicators_only)?,
                    'g' => ri.fmt_branch_glyph(buf, &styles.branch_glyph)?,
                    'm' => ri.unstaged.fmt_modified(
                        buf,
                        &styles.modified_unstaged,
                        args.indicators_only,
                    )?,
                    'n' => write!(buf, "{}git", styles.plain)?,
                    'r' => ri.fmt_upstream(buf, &styles.upstream)?,
                    's' => ri.staged.fmt_modified(
                        buf,
                        &styles.modified_staged,
                        args.indicators_only,
                    )?,
                    't' => ri.fmt_stash(buf, &styles.stash, args.indicators_only)?,
                    'u' => ri.fmt_untracked(buf, &styles.untracked, args.indicators_only)?,
                    'U' => ri.fmt_unmerged(buf, &styles.unmerged, args.indicators_only)?,
                    '%' => write!(buf, "{}%", styles.plain)?,
                    _ => unreachable!(
                        "invalid format token allowed to reach print_output: \"%{}\"",
                        c
                    ),
                }
            }
        } else {
            if c != ' ' {
                // Only write plain style if there's something there
                styles.plain.write_to(buf)?;
            }
            write!(buf, "{}", c)?;
        }
    }
    Ok(())
}

/// Entry point
fn main() -> Result {
    let args = Arg::parse();
    let mut opts: Opt = Default::default();

    if !args.quiet {
        logger::init_logger(args.verbose);
    }
    if args.no_color {
        env::set_var("NO_COLOR", "1");
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
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        lock.write_all(&buf)?;
        return Ok(());
    }
    if args.simple_mode2 {
        let mut buf = Vec::with_capacity(255);
        simple_output2(&mut buf)?;
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

    let mut buf = vec![];
    print_output(ri, &args, &mut buf)?;
    let out = if args.no_trim {
        String::from_utf8(buf)?
    } else {
        String::from_utf8(buf)?
            .to_string()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    debug!("{:?}", out);
    let test = out.split_whitespace().collect::<Vec<_>>();
    debug!("{:?}", test);
    print!("{}", out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn simple_clean() -> Result {
        const CLEAN: &str = "## master...origin/master";
        let expected = "\u{1b}[38;5;14m(master)\u{1b}[0m";

        let mut buf = Vec::new();
        simple_output(CLEAN, &mut buf)?;
        let result = str::from_utf8(&buf)?;
        assert_eq!(result, expected);
        Ok(())
    }

    #[test]
    fn simple_dirty() -> Result {
        const DIRTY: &str = "## master...origin/master
  M src/main.rs
 ?? src/tests.rs";
        let expected = "\u{1b}[38;5;14m(master)\u{1b}[31m*\u{1b}[0m";

        let mut buf = Vec::new();
        simple_output(DIRTY, &mut buf)?;
        let result = str::from_utf8(&buf)?;
        assert_eq!(result, expected);
        Ok(())
    }

    #[test]
    fn absolute_git_dir() -> Result {
        let fs_dir =
            find_git_dir(&env::current_dir()?).ok_or_else(|| format_err!("cannot find git dir"))?;
        let git_dir = cmd!("git", "rev-parse", "--absolute-git-dir").read()?;
        assert_eq!(git_dir, fs_dir.to_string_lossy());
        Ok(())
    }
}
