## Summary

<!-- What changes, and why. Link the issue this closes, for example "Closes #123". -->

## Type of change

- [ ] Bug fix
- [ ] Feature
- [ ] Documentation only
- [ ] Refactor or test only
- [ ] Build, CI or dependency update

## Checks run

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `uv sync --locked`
- [ ] `uv run ruff check`
- [ ] `uv run pytest`
- [ ] `uv run python scripts/check_python_licenses.py`
- [ ] `uv run python scripts/audit_headers.py`
- [ ] Optional Tier 2 tools tested (list them below), or not applicable

<!-- List the optional tools you ran, if any. -->

## Data and privacy

- [ ] This change does not alter what Lucubra reads, writes or sends.
- [ ] If it does, [DATA_HANDLING.md](../DATA_HANDLING.md) is updated in this pull request, and the change is described below.

<!-- Describe any change to data paths, network behaviour or host output. -->

## Hosts and budgets

- [ ] This change does not affect host detection, plan-tier budgets or tool output shape.
- [ ] If it does, the host pages in `docs/hosts/` are updated, and the affected hosts are listed below.

## Documentation and changelog

- [ ] `CHANGELOG.md` is updated under Unreleased, if the change is user-visible.
- [ ] Documentation that describes this behaviour is updated.

## Commits

- [ ] Commit messages follow Conventional Commits.
- [ ] No attribution lines are included in commit messages.
- [ ] Documentation and configuration changes use one commit per file.

## Notes for reviewers

<!-- Anything reviewers should look at first, known gaps, or follow-up work. -->
