# brew-rs

`brew-rs` is the opt-in Rust frontend for a small set of `brew` commands.
Today the gated command set is `search`, `info`, `list`, `install`,
`reinstall`, `update`, `upgrade`, and `uninstall`.

Meaningful Rust behavior currently exists for `search` and `list`.
`info` and the mutating commands currently print a warning to `stderr` and
delegate back to the existing Ruby frontend for correctness.

It is intentionally built through `brew vendor-install brew-rs`, which runs
standard Cargo commands under the hood instead of a bespoke build wrapper.

## Porting Rule

When Rust takes ownership of behavior that already exists in Homebrew Ruby or
Bash, prefer to mirror the comparable Homebrew source file and filename when it
is practical. Keep command entrypoints in `src/cmd/` for
`Library/Homebrew/cmd/*.rb` and in `src/dev_cmd/` for
`Library/Homebrew/dev-cmd/*.rb`. For example, keep work that corresponds to
`Library/Homebrew/download_queue.rb` in `src/download_queue.rs`.

Treat that filename mirroring as the default for new Rust ports. Only diverge
when the Homebrew side has no clear single-file analogue or when a Rust-specific
split keeps the code materially simpler. When Rust and Homebrew already share a
clear concept name, prefer matching the Rust module, function, and type names
too, for example `Search.search_names` to `search::search_names`.

## Pre-step

Export both Rust frontend gate variables before building or running `brew-rs`:

```bash
export HOMEBREW_DEVELOPER=1
export HOMEBREW_EXPERIMENTAL_RUST_FRONTEND=1
```

From the repository root, before running `rake`, add Homebrew's portable Ruby to
`PATH`:

```bash
portable_ruby_bindir="$PWD/Library/Homebrew/vendor/portable-ruby/current/bin"
[[ -x "${portable_ruby_bindir}/ruby" ]] || ./bin/brew vendor-install ruby
export PATH="${portable_ruby_bindir}:${PATH}"
```

## Build

From the repository root:

```bash
./bin/brew vendor-install brew-rs
```

## Enable

```bash
./bin/brew search jq
```

The first supported Rust-backed command will run `brew vendor-install brew-rs`
automatically and skip rebuilding when the vendored binary is already up-to-date.
If `cargo` from the `rust` formula is not available yet, `brew` prints a warning
to `stderr` and falls back to the existing Ruby frontend instead of failing.
When `brew-rs` is used, `brew` prints an experimental warning to `stderr`.
If a command is not meaningfully implemented in Rust yet, `brew-rs` also
prints a handoff warning before delegating back to Ruby.

## Rust Checks

```bash
cd Library/Homebrew/rust/brew-rs
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

## Homebrew Checks

```bash
HOMEBREW_NO_AUTO_UPDATE=1 ./bin/brew typecheck
HOMEBREW_NO_AUTO_UPDATE=1 ./bin/brew lgtm
```

The Rust frontend tests now live in the `brew-rs` crate and the dedicated
`brew-rs` GitHub Actions workflow instead of `brew tests`.

## Benchmark

The `benchmark` task compares the Ruby and Rust frontends with `hyperfine` for
the commands currently gated through `brew-rs`.

`rake` should be run with Homebrew's portable Ruby bin directory at the front
of `PATH`. The benchmark prints the normal `hyperfine` output along with each
command's stdout/stderr. Right now it only benchmarks commands with meaningful
Rust implementations: `search` and `list <installed formula>`.

It prefers the workflow-installed `rust` formula for the `list` benchmark and
falls back to the first installed formula if `rust` is unavailable. If the
vendored binary is missing, the benchmark task builds it first with
`vendor-install`.

```bash
brew install hyperfine
cd Library/Homebrew/rust/brew-rs
rake benchmark
```

## Tier 1 Smoke Test

Run these on a default-prefix Tier 1 Homebrew install outside this repository checkout:

```bash
brew install hello
brew reinstall hello
brew upgrade hello
brew uninstall hello
brew update --quiet --force
```

Current install smoke test:
`aview` was the real dependency-bearing formula used to validate the
current Rust install path because its `aalib` dependency stays inside the
supported bottle-only slice. `libaacs` still delegates back to Ruby
because it has `build_dependencies`, `uses_from_macos`, `post_install`,
and a `:any` bottle that still needs relocation support. I did not find a
current real formula with a `>=2` dependency tree that also stays inside
the current Rust install boundary, so `aview` is the closest real-world
smoke test for now. In this environment I had to prime the cache with
Ruby `brew fetch aalib aview` first because Ruby could fetch those GHCR
bottles while the Rust downloader still failed against GHCR.

## Next Steps

The current plan is to keep correctness-sensitive install lifecycle work in
Ruby while moving the heavy bottle path into Rust in small slices.

- Keep `brew.sh` as a thin gate and dispatch layer.
- Keep using existing Homebrew paths, caches, and metadata formats.
- Keep command entrypoints under `src/cmd/` and `src/dev_cmd/` so they mirror
  `Library/Homebrew/cmd/` and `Library/Homebrew/dev-cmd/`.
- Mirror the comparable Homebrew filename in Rust when practical, for example
  `download_queue.rb` to `download_queue.rs`, so ports stay easy to compare.
- Match Rust module, function, and type names to the comparable Homebrew names
  when the mapping is clear and does not make the Rust code worse.
- Move more read-only behavior into Rust when it produces clear wins.
- For formula installs, move bottle fetch and pour into Rust first.
- Keep `post_install` and the more involved finish/finalization steps in Ruby
  until narrower shared helpers exist and profiling shows a clear benefit to
  moving more of that logic.
