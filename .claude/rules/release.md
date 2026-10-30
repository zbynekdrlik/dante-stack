---
paths:
  - "Cargo.toml"
  - "Cargo.lock"
  - "CHANGELOG.md"
  - "fuzz/Cargo.toml"
  - "fuzz/Cargo.lock"
  - ".github/workflows/**"
---

# Releasing dante-stack

- **Bump first.** `[workspace.package].version` in `Cargo.toml` is the first commit of every PR (`scripts/check_version.py` fails a PR not above main). Rebuild `fuzz/` too: its lockfile records the stack's version and CI checks it with `cargo metadata --locked`.
- **Merge** a green, clean PR with `gh api -X PUT repos/zbynekdrlik/dante-stack/pulls/<N>/merge -f merge_method=merge`. `gh pr merge` once failed in GraphQL after the merge commit landed, leaving the PR open with merged=false.
- **Tag** the merge commit on main, annotated: `git tag -a vX.Y.Z -m "..." origin/main`, push the tag, then fast-forward `dev` to main and push it. Watch the main and tag CI runs to green.
- **CHANGELOG** names every change an application can notice, not only new API: `stream::BLOCKS` widening broke iemmixer's RT tests once without a changelog line. A removed or changed public item, a JSON field, a default: listed, with what a consumer ports.
- **Tell the consumers** over peer (iemmixer, dante-asio, songplayer as it applies) with the tag and what they must change.
- **New dependencies** must be published at least two weeks ago (`curl https://crates.io/api/v1/crates/<c>/<v>` gives `created_at`); pin older with `cargo update -p <c> --precise <v>`. Seed a new lockfile (fuzz) from the main `Cargo.lock` so tested versions carry over.
- **Public repo:** no site addresses or real device names in code, commits, issues or PR text; they live in `~/.config/dante-stack/`.
