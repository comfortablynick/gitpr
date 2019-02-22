use log::{info, trace};
use std::{
    env, io,
    path::PathBuf,
    process::{Command, Output, Stdio},
    str,
};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "gitpr",
    about = "git repo status for shell prompt",
    raw(setting = "structopt::clap::AppSettings::ColoredHelp")
)]
struct Opt {
    /// Debug verbosity (ex: -v, -vv, -vvv)
    #[structopt(
        short = "v",
        long = "verbose",
        // default_value = "2",
        parse(from_occurrences)
    )]
    verbose: u8,

    /// Format print-f style string
    #[structopt(
        short = "f",
        long = "format",
        // default_value = "%g (%b@%c) %a %m%d %u%t %s",
        default_value = "%g %b@%c %a %m %u %s",
        long_help = "Tokenized string may contain:
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
    %t  stashed files indicator
"
    )]
    format: String,

    #[structopt(short = "d", long = "dir", default_value = ".")]
    dir: String,
}

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

    fn git_root_dir(&mut self) -> String {
        let cmd = exec("git rev-parse --absolute-git-dir")
            .expect("error calling `git rev-parse --absolute-git-dir`");
        let output = String::from_utf8(cmd.stdout).ok();
        self.git_dir = output.clone();
        output.unwrap_or_default().trim().to_string()
    }

    fn git_diff_numstat(&mut self) {
        let cmd = exec("git diff --numstat").expect("error calling `git diff --numstat`");
        let output = String::from_utf8(cmd.stdout).unwrap_or_default();
        for line in output.lines() {
            let mut split = line.split_whitespace();
            self.insertions += split.next().unwrap_or_default().parse().unwrap_or(0);
            self.deletions += split.next().unwrap_or_default().parse().unwrap_or(0);
        }
    }

    /* Parse git status by line */
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
                return s[..len].to_string();
            }
            None => String::new(),
        }
    }

    fn fmt_ahead_behind(&self) -> String {
        let mut out = String::new();
        if self.ahead != 0 {
            out.push_str(&format!("{}{}", Repo::AHEAD_GLYPH, self.ahead));
        }
        if self.behind != 0 {
            out.push_str(&format!("{}{}", Repo::BEHIND_GLYPH, self.behind));
        }
        out
    }

    fn fmt_diff_numstat(&mut self) -> String {
        self.git_diff_numstat();
        let mut out = String::new();
        if self.insertions + self.deletions != 0 {
            out.push_str(&format!("+{}/-{}", self.insertions, self.deletions));
        }
        out
    }

    fn fmt_stash(&mut self) -> String {
        let mut out = String::new();
        let mut git = match self.git_dir.clone() {
            Some(d) => d,
            None => self.git_root_dir(),
        };
        git.push_str("/logs/refs/stash");
        let st = std::fs::read_to_string(git)
            .unwrap_or_default()
            .lines()
            .count();
        if st > 0 {
            self.stashed = st as u32;
            out.push_str(&format!("{}{}", Repo::STASH_GLYPH, st));
        }
        out
    }

    fn fmt_untracked(&self) -> String {
        let mut out: String = String::new();
        if self.untracked != 0 {
            out.push_str(&format!("{}{}", Repo::UNTRACKED_GLYPH, self.untracked));
        }
        out
    }
}

impl GitArea {
    fn parse_modified(&mut self, ln: char) -> () {
        match ln {
            'M' => self.modified += 1,
            'A' => self.added += 1,
            'D' => self.deleted += 1,
            'R' => self.renamed += 1,
            'C' => self.copied += 1,
            _ => (),
        }
    }

    fn fmt_modified(&self) -> String {
        let mut out: String = String::new();
        if self.modified != 0 {
            out.push_str(&format!("{}{}", Repo::MODIFIED_GLYPH, self.modified));
        }
        out
    }
}

fn exec(cmd: &str) -> io::Result<Output> {
    let args: Vec<&str> = cmd.split_whitespace().collect();
    let command = Command::new(&args[0])
        .args(args.get(1..).expect("missing args in cmd"))
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

fn main() -> io::Result<()> {
    let opts = Opt::from_args();

    env::set_var(
        "RUST_LOG",
        match &opts.verbose {
            0 => "warning",
            1 => "info",
            2 | _ => "trace",
        },
    );
    env::set_current_dir(&opts.dir)?;
    env_logger::init();

    // TODO: possibly use rev-parse first, kill 2 birds?
    let git_status = exec("git status --porcelain=2 --branch")?;
    let mut ri = Repo::new();
    ri.parse_status(
        str::from_utf8(&git_status.stdout)
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "Error"))?,
    );

    // parse fmt string
    let mut fmt_str = opts.format.chars();
    let mut out: String = String::new();
    while let Some(c) = fmt_str.next() {
        if c == '%' {
            if let Some(c) = fmt_str.next() {
                match &c {
                    ' ' => out.push(' '),
                    'a' => out.push_str(&ri.fmt_ahead_behind().as_str()),
                    'b' => out.push_str(&ri.fmt_branch().as_str()),
                    'c' => out.push_str(&ri.fmt_commit(7).as_str()),
                    'd' => out.push_str(&ri.fmt_diff_numstat().as_str()),
                    'g' => out.push(Repo::BRANCH_GLYPH),
                    'm' => out.push_str(&ri.unstaged.fmt_modified().as_str()),
                    'n' => out.push_str("git"),
                    's' => out.push_str(&ri.staged.fmt_modified().as_str()),
                    't' => out.push_str(&ri.fmt_stash().as_str()),
                    'u' => out.push_str(&ri.fmt_untracked().as_str()),
                    '%' => out.push('%'),
                    &c => panic!("Invalid flag: \"%{}\"", &c),
                }
            }
        } else {
            out.push(c);
        }
    }
    trace!("{:#?}", &ri);
    info!("{:#?}", &opts);

    println!("{}", &out);
    Ok(())
}
