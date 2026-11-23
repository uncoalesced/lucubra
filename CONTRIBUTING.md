# Contributing to Lucubra

Thank you for considering a contribution. This guide covers the development setup, the checks that CI runs, how to write commits and how to report problems.

Read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) first. Report security issues through [SECURITY.md](SECURITY.md), not through a public issue.

## Requirements

- Rust, stable toolchain, with `rustfmt` and `clippy`. Install with [rustup](https://rustup.rs).
- Python 3.11 or later. Lucubra no longer supports Python 3.10.
- [uv](https://docs.astral.sh/uv/) for Python dependencies and the Python test runner.
- Git.

## Development setup

Clone the repository and build the Rust workspace:

```sh
git clone https://github.com/uncoalesced/lucubra.git
cd lucubra
cargo build --workspace
```

Install the Python environment from the lock file:

```sh
uv python install 3.11
uv sync --locked
```

Run the CLI from the build tree:

```sh
cargo run -p lucubra-cli -- --version
```

### Isolated data directory for tests

Tests write state to the application-data directory. Point it at a scratch folder so that your own state is not touched:

```sh
export LUCUBRA_DATA_DIR="$PWD/target/test-data"   # POSIX shells
```

```powershell
$env:LUCUBRA_DATA_DIR = "$PWD\target\test-data"   # PowerShell
```

## Checks that CI runs

CI runs the commands below on every push and pull request (`.github/workflows/ci.yml`). Run them locally before you open a pull request. Run them in this order:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
uv sync --locked
uv run ruff check
uv run pytest
uv run python scripts/check_python_licenses.py
uv run python scripts/audit_headers.py
```

The CI job `deny` also runs `cargo deny check licenses bans sources`. Install `cargo-deny` and run that command when you add or update a Rust dependency.

The wheel job builds the package with `uvx maturin build --release --out dist` on Linux, Windows and macOS, and runs `lucubra --version` from the wheel. You can reproduce that build locally.

## Running tests

- `cargo test --workspace` runs the Rust unit and integration tests. Some MCP transcript tests start the `lucubra` binary, so they build it first.
- `uv run pytest` runs the Python worker tests. They use the fixture corpus in `tests/fixtures/`, whose manifest is `tests/fixtures/MANIFEST.json`.
- The default test run does not need the optional Tier 2 tools (ODA File Converter, LibreDWG or Blender). Say in your pull request which of them you ran.

## Source file headers

Every source code file (`.rs`, `.py`, `.ps1`, `.sh`, `.js`, `.mjs` and `.ts`) starts with two comment lines, within its first five lines:

1. an SPDX licence identifier line for `Apache-2.0`;
2. the provenance comment set out in [ADR 0007](docs/adr/0007-source-header-policy.md), written in the file's own comment syntax.

Documentation, configuration, lock files, data and fixtures carry neither line. `scripts/audit_headers.py` checks these rules, and CI fails when a rule is broken. Do not copy the provenance text into any file other than a source code file.

## Commit conventions

- Use [Conventional Commits](https://www.conventionalcommits.org/): `type(scope): summary`, for example `fix(ingest): run Blender with autoexec disabled`.
- Common types: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`, `ci`, `build`.
- Keep the summary under 72 characters, in the imperative mood, without a final full stop.
- Make one commit per file when you change documentation or configuration. Make one commit per logical change for code, with its tests in the same commit.
- Do not add attribution lines, such as `Co-authored-by` or tool credits, to commit messages.

## Pull requests

- Branch from `main`. Name the branch after the change, for example `fix/blender-autoexec`.
- Fill in the pull request template. It asks which checks you ran and whether your change touches data handling.
- A change that alters what data is read, written or sent must update [DATA_HANDLING.md](DATA_HANDLING.md) in the same pull request.
- Keep pull requests focused. Open a separate pull request for unrelated cleanup.

## Reporting problems

Use the issue templates in `.github/ISSUE_TEMPLATE/`:

- **Bug report** for a defect in Lucubra.
- **Feature request** for a change in behaviour or a new capability.
- **Host compatibility report** when an AI host does not connect, or does not behave as documented in the host pages in `docs/hosts/`.

Do not paste study material, grades, personal details or credentials into an issue. Describe the problem and include the error text, and check any file you attach before you post it.

## Licence

By contributing, you agree that your contribution is licensed under the Apache License, Version 2.0, the licence of this project.
