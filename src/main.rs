//! Print git repo status. Handy for shell prompt.
mod logger;

#[allow(unused_imports)]
use ansi_term::{
    Color::Fixed,
    {ANSIString, ANSIStrings},
};
use anyhow::Context;
use clap::{AppSettings, ArgSettings, Clap};
use log::{debug, info, warn};
use std::{
    env,
    io::Write,
    path::PathBuf,
    process::{self, Command, Output, Stdio},
    str,
};
use termcolor::{Buffer, Color, ColorChoice, ColorSpec, WriteColor};

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
/// Blue ANSI color
const BLUE: u8 = 12;
/// Red ANSI color
const RED: u8 = 124;
/// Cyan ANSI color (intense)
const CYAN: u8 = 14;
/// Bold silver ANSI color
const BOLD_SILVER: u8 = 188;

/// Options from format string
#[derive(Debug, Default)]
struct Opt {
    show_ahead_behind: bool,
    show_branch: bool,
    show_branch_glyph: bool,
    show_commit: bool,
    show_diff: bool,
    show_upstream: bool,
    show_stashed: bool,
    show_staged_modified: bool,
    show_unstaged_modified: bool,
    show_untracked: bool,
    show_unmerged: bool,
    show_vcs: bool,
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
#[derive(Debug, Default)]
struct GitArea {
    modified: u32,
    added: u32,
    deleted: u32,
    renamed: u32,
    copied: u32,
}

impl Repo {
    const AHEAD_GLYPH: char = '⇡';
    const BEHIND_GLYPH: char = '⇣';
    const BRANCH_GLYPH: char = '';
    const MODIFIED_GLYPH: char = 'Δ';
    const STASH_GLYPH: char = '$';
    const UNMERGED_GLYPH: char = '‼';
    const UNTRACKED_GLYPH: char = '…';

    // const DIRTY_GLYPH: char = '✘';
    // const CLEAN_GLYPH: char = '✔';

    // TODO: simplify this -- does it have to be written to the Repo struct?
    fn git_root_dir(&mut self) -> Result<String> {
        if let Some(dir) = self.git_base_dir.clone() {
            return Ok(dir);
        }
        let cmd = exec(&["git", "rev-parse", "--absolute-git-dir"])?;
        let output = String::from_utf8(cmd.stdout)?;
        self.git_base_dir = Some(output.clone());
        Ok(output.trim().to_string())
    }

    /// Get chunk insertions/deletions
    fn git_diff_numstat(&mut self) -> Result {
        let cmd = exec(&["git", "diff", "--numstat"])?;
        let output = String::from_utf8(cmd.stdout)?;
        for line in output.lines() {
            let mut split = line.split_whitespace();
            self.insertions += split.next().unwrap_or_default().parse().unwrap_or(0);
            self.deletions += split.next().unwrap_or_default().parse().unwrap_or(0);
        }
        Ok(())
    }

    /// Parse git status by line
    fn parse_status(&mut self, gs: &str) {
        for line in gs.lines() {
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
    fn fmt_branch(&self, buf: &mut Buffer) -> Result {
        if let Some(s) = &self.branch {
            buf.set_color(ColorSpec::new().set_fg(Some(Color::Ansi256(BLUE))))?;
            write!(buf, "{}", s)?;
        }
        Ok(())
    }

    /// Write branch glyph to buffer
    fn fmt_branch_glyph(&self, buf: &mut Buffer) -> Result {
        buf.set_color(&ColorSpec::new())?;
        write!(buf, "{}", Repo::BRANCH_GLYPH)?;
        Ok(())
    }

    /// Write formatted commit to buffer
    fn fmt_commit(&self, buf: &mut Buffer, len: usize) -> Result {
        match &self.commit {
            Some(s) => {
                buf.set_color(
                    ColorSpec::new()
                        .set_fg(Some(Color::Black))
                        .set_bg(Some(Color::Green)),
                )?;
                if s == "(initial)" {
                    write!(buf, "(initial)")?;
                } else {
                    write!(buf, "{}", s[..len].to_string())?;
                }
            }
            None => (),
        }
        Ok(())
    }

    /// Write formatted ahead/behind details to buffer
    fn fmt_ahead_behind(&self, buf: &mut Buffer, indicators_only: bool) -> Result {
        if self.ahead != 0 {
            write!(buf, "{}", Repo::AHEAD_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", &self.ahead)?;
            }
        }
        if self.behind != 0 {
            write!(buf, "{}", Repo::BEHIND_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", self.behind)?;
            }
        }
        Ok(())
    }

    /// Write formatted +n/-n git diff numstat details to buffer
    fn fmt_diff_numstat(&mut self, buf: &mut Buffer, indicators_only: bool) -> Result {
        if !self.unstaged.has_changed() || indicators_only {
            return Ok(());
        }
        buf.set_color(ColorSpec::new().set_fg(Some(Color::Ansi256(BOLD_SILVER))))?;
        if self.insertions == 0 && self.deletions == 0 {
            self.git_diff_numstat()?;
        }
        if self.insertions > 0 {
            write!(buf, "+{}", self.insertions)?;
            if self.deletions > 0 {
                write!(buf, "/")?;
            }
            if self.deletions > 0 {
                write!(buf, "-{}", self.deletions)?;
            }
        }
        Ok(())
    }

    /// Write formatted stash details to buffer
    fn fmt_stash(&mut self, buf: &mut Buffer, indicators_only: bool) -> Result {
        let mut git = self.git_root_dir()?;
        git.push_str("/logs/refs/stash");
        let st = std::fs::read_to_string(git)
            .unwrap_or_default()
            .lines()
            .count();
        if st > 0 {
            self.stashed = st as u32;
            buf.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
            write!(buf, "{}", Repo::STASH_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", st)?;
            }
        }
        Ok(())
    }

    /// Write formatted untracked indicator and/or count to buffer
    fn fmt_untracked<T>(&mut self, buf: &mut T, indicators_only: bool) -> Result
    where
        T: std::io::Write + termcolor::WriteColor,
    {
        if self.untracked > 0 {
            buf.set_color(ColorSpec::new().set_fg(Some(Color::Ansi256(245))))?;
            write!(buf, "{}", Repo::UNTRACKED_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", self.untracked)?;
            }
        }
        Ok(())
    }

    /// Write formatted unmerged files indicator and/or count to buffer
    fn fmt_unmerged(&mut self, buf: &mut Buffer, indicators_only: bool) -> Result {
        if self.unmerged > 0 {
            buf.set_color(ColorSpec::new().set_fg(Some(Color::Ansi256(196))))?;
            write!(buf, "{}", Repo::UNMERGED_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", self.unmerged)?;
            }
        }
        Ok(())
    }

    /// Write formatted upstream to buffer
    fn fmt_upstream(&self, buf: &mut Buffer) -> Result {
        if let Some(r) = &self.upstream {
            buf.set_color(&ColorSpec::new())?;
            write!(buf, "{}", r)?;
        }
        Ok(())
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

    fn fmt_modified(&self, buf: &mut Buffer, indicators_only: bool) -> Result {
        if self.has_changed() {
            buf.set_color(ColorSpec::new().set_fg(Some(Color::Ansi256(RED))))?;
            write!(buf, "{}", Repo::MODIFIED_GLYPH)?;
            if !indicators_only {
                write!(buf, "{}", self.change_ct())?;
            }
        }
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
    let cmd = exec(&["git", "describe", "--tags", "--exact-match"])?;
    let tag = str::from_utf8(&cmd.stdout)?.trim_end().to_string();
    Ok(tag)
}

/// Spawn subprocess for `cmd` and access stdout/stderr
fn exec(command: &[&str]) -> Result<Output> {
    let mut cmd = command.into_iter();
    let mut spawn = Command::new(cmd.next().context("exec: command missing")?);
    while let Some(arg) = cmd.next() {
        spawn.arg(arg);
    }
    let result = spawn
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .wait_with_output()
        .with_context(|| format!("Command failed: [{:?}]", spawn))?;

    if !result.status.success() {
        warn!(
            "Command returned non-zero status: [{:?}]; Result: {:?}",
            spawn, result
        );
    } else {
        debug!("Command: [{:?}]; Result: {:?}", spawn, result);
    }
    Ok(result)
}

/// Simple output to mimic default git prompt
fn simple_output(git_status: &str) -> Result<String> {
    let mut raw_branch = "";
    let mut dirty = false;
    for line in git_status.lines() {
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
    let mut strings: Vec<ANSIString> = vec![
        Fixed(CYAN).paint("("),
        Fixed(CYAN).paint(branch),
        Fixed(CYAN).paint(")"),
    ];
    if dirty {
        strings.push(Fixed(RED).paint("*"));
    }
    Ok(ANSIStrings(&strings).to_string())
}

/// Print output based on parsing of --format string
fn print_output(mut ri: Repo, args: Arg, buf: &mut Buffer) -> Result {
    let mut fmt_str = args.format.chars();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match c {
                    'a' => ri.fmt_ahead_behind(buf, args.indicators_only)?,
                    'b' => ri.fmt_branch(buf)?,
                    'c' => ri.fmt_commit(buf, 7)?,
                    'd' => ri.fmt_diff_numstat(buf, args.indicators_only)?,
                    'g' => ri.fmt_branch_glyph(buf)?,
                    'm' => ri.unstaged.fmt_modified(buf, args.indicators_only)?,
                    'n' => write!(buf, "git")?,
                    'r' => ri.fmt_upstream(buf)?,
                    's' => ri.staged.fmt_modified(buf, args.indicators_only)?,
                    't' => ri.fmt_stash(buf, args.indicators_only)?,
                    'u' => ri.fmt_untracked(buf, args.indicators_only)?,
                    'U' => ri.fmt_unmerged(buf, args.indicators_only)?,
                    '%' => write!(buf, "%")?,
                    _ => panic!("print_output: invalid flag: \"%{}\"", c),
                }
            }
        } else {
            buf.set_color(&ColorSpec::new())?;
            write!(buf, "{}", c)?;
        }
    }
    writeln!(buf)?;
    Ok(())
}

/// Entry point
fn main() -> Result {
    let args = Arg::parse();
    let mut opts: Opt = Default::default();
    let bufwtr = if args.no_color {
        termcolor::BufferWriter::stdout(ColorChoice::Never)
    } else {
        termcolor::BufferWriter::stdout(ColorChoice::Auto)
    };
    let mut buf = bufwtr.buffer();

    if !args.quiet {
        logger::init_logger(args.verbose);
    }

    if args.no_color {
        env::set_var("TERM", "dumb");
    };

    env::set_current_dir(&args.dir)?;

    if args.simple_mode {
        let status_cmd = exec(&[
            "git",
            "status",
            "--porcelain",
            "--branch",
            "--untracked-files=no",
        ])?;
        let status = str::from_utf8(&status_cmd.stdout)?;
        println!("{}", simple_output(status)?);
        return Ok(());
    }
    // TODO: use env vars for format str and glyphs
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
                    'r' => opts.show_upstream = true,
                    's' => opts.show_staged_modified = true,
                    't' => opts.show_stashed = true,
                    'u' => opts.show_untracked = true,
                    'U' => opts.show_unmerged = true,
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

    // TODO: possibly use rev-parse first
    let mut git_args = [
        "git",
        "status",
        "--porcelain=2",
        "--branch",
        "--untracked-files=no",
    ];
    if opts.show_untracked {
        git_args[4] = "--untracked-files=normal";
    }
    debug!("Cmd: {:?}", git_args);

    let git_status = exec(&git_args)?;
    let mut ri = Repo::default();
    ri.parse_status(str::from_utf8(&git_status.stdout)?);

    debug!("{:#?}", &ri);
    info!("{:#?}", &args);

    print_output(ri, args, &mut buf)?;
    bufwtr.print(&buf)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_simple_clean() -> Result {
        const CLEAN: &str = "## master...origin/master";
        let expected = "\u{1b}[38;5;14m(master)\u{1b}[0m";

        let result = simple_output(CLEAN)?;
        assert_eq!(result, expected);
        Ok(())
    }

    #[test]
    fn test_simple_dirty() -> Result {
        const DIRTY: &str = "## master...origin/master
 M src/main.rs
?? src/tests.rs";
        let expected = "\u{1b}[38;5;14m(master)\u{1b}[38;5;124m*\u{1b}[0m";

        let result = simple_output(DIRTY)?;
        assert_eq!(result, expected);
        Ok(())
    }
}
