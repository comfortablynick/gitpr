#!/usr/bin/env just --justfile
bin_name := "gitpr"
alias r := run
alias b := build
alias i := install
alias h := help
alias lh := longhelp
alias q := runq

dev := '1'

# automatically build on each change
autobuild:
    cargo watch -x build

# build release binary
build:
    cargo build

# rebuild docs
doc:
    cargo doc

# rebuild docs and start simple static server
docs +PORT='40000':
    cargo doc && http target/doc -p {{PORT}}

# start server for docs and update upon changes
docslive:
    light-server -c .lightrc

# rebuild docs and start simple static server that watches for changes (in parallel)
docw +PORT='40000':
    parallel --lb ::: "cargo watch -x 'doc --color=always'" "http target/doc -p {{PORT}}"

# install binary to ~/.cargo/bin
install:
    cargo install --path . -f

# build release binary and run
run +args='':
    cargo run -- {{args}}

# run with --quiet
runq:
    cargo run -- -q

# run with -v
runv:
    cargo run -- -v

# run with -vv
runvv:
    cargo run -- -vv

# clap short help
help:
    cargo run -- -h

# clap long help
longhelp:
    cargo run -- --help

# run binary
rb +args='':
    ./target/debug/{{bin_name}} {{args}}

test:
    cargo test

fix:
    cargo fix
