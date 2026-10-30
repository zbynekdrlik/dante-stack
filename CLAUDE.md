# dante-stack

The one Dante expert project. All Dante code lives here: discovery, ARC, CMC, flows, media TX/RX, clock client, health. Applications depend on a tagged release by Cargo git tag; nobody keeps a copy or a local patch.

Start from `docs/HANDOVER.md`, then `docs/design/2026-10-30-shared-dante-stack.md`.

- Your job: own every Dante problem and make the stack robust, so no project solves a Dante problem twice.
- A Dante bug reported by a consumer is fixed here, with a catalog test, and released; the consumer only moves the version.
- A field bug is closed only with a recorded capture and a replay test (quirk catalog).
- Branches: `dev` and `main` only; PRs dev → main; merge commits.
- Public repo: no site data. Addresses and real device or channel names live only in `~/.config/dante-stack/`.
- dantesync is the only clock owner on a PC; the stack never opens UDP 319/320.
- Never change another device's subscriptions without the owner's approval. Tests subscribe our own device to itself.
- Every message to the owner in Slovak; code, commits and docs in English.

## Playbook router
- releases, versions, CHANGELOG, dependencies → `.claude/rules/release.md` (auto-loads on its `paths:`)
- tests, whole-device tests, the mutation gate → `.claude/rules/tests.md` (auto-loads on its `paths:`)
