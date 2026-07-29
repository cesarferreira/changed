<div align="center">
  <h1>changed</h1>

  <p><strong>Live read-only TUI showing what changed in your git worktree</strong></p>

  <p>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
    <img alt="Rust" src="https://img.shields.io/badge/rust-1.85%2B-orange">
    <img alt="Edition" src="https://img.shields.io/badge/edition-2024-blue">
    <a href="https://crates.io/crates/changed-cli"><img alt="crates.io" src="https://img.shields.io/crates/v/changed-cli.svg"></a>
  </p>

  <p>
    <a href="#install">Install</a>
    &nbsp;·&nbsp;
    <a href="#quickstart">Quickstart</a>
    &nbsp;·&nbsp;
    <a href="#development">Development</a>
  </p>
</div>

---

## Install

Requires [Rust](https://rustup.rs) **1.85+** and `~/.cargo/bin` on your `PATH`.

```bash
cargo install changed-cli
```

Verify:

```bash
changed --help
```

<details>
<summary><strong>Build from source</strong> — for development or unreleased changes</summary>

```bash
git clone https://github.com/cesarferreira/changed.git
cd changed
cargo install --path . --locked
# or
make install-release
```

Debug install (faster compile, larger binary):

```bash
make install
```

Run without installing:

```bash
make build-release
./target/release/changed
```

</details>

<a id="quickstart"></a>
## Quickstart

```bash
changed --help
```

<a id="performance"></a>
## Performance on large repos

`changed` watches the whole worktree, but only re-queries git when an event
can actually change `git status` output:

- Events under git-ignored paths (build output, `node_modules`, …) are
  filtered out, using the repo's `.gitignore` files and `.git/info/exclude`.
- Git-internal churn is limited to `HEAD`, `index`, `packed-refs`, `refs/**`
  and `*_HEAD` — object store and gc activity never triggers a refresh.
- Refreshes are debounced (120 ms) and rate-limited (750 ms) during sustained
  bursts.

For very large repos and monorepos, git-side caches make each refresh
dramatically cheaper:

```bash
git config core.untrackedCache true
git config core.fsmonitor true
```

<a id="development"></a>
## Development

Common tasks via the `Makefile`:

```bash
make              # check + build + test
make build        # debug build
make build-release
make install      # install debug binary
make install-release
make run ARGS="--hello"
make check        # cargo check + clippy
make fmt          # format
make lint         # fmt check + clippy
make test
make clean
make demo         # install + show --help
```

Releasing (requires [cargo-release](https://github.com/crate-ci/cargo-release)):

```bash
make release                  # default minor bump
make release LEVEL=patch      # patch bump
make release LEVEL=major      # major bump
```

The pre-release hook finalizes `CHANGELOG.md` from commits since the latest `v*` tag, refreshes compare links, and leaves a fresh `Unreleased` header. If there are no commits since the last tag, the release stops before publishing.

## License

MIT
