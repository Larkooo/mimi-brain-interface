# Evidence-driven maintenance

Mimi observes continuously and acts only on evidence. It does not need to produce
a PR every night. An idle assistant is not automatically a broken assistant.

## Commands and state

- `mimi maintain` (alias of `mimi audit`): collect evidence and write a local report;
  never deploy or merge. A changed-evidence model brief runs at most weekly.
- `mimi maintain --apply`: recover interrupted releases, monitor probation, and
  validate/deploy/merge one eligible PR. `mimi update` uses this same safe path.
- `mimi maintain --review`: explicitly refresh the advisory brief, overriding its
  cooldown. The reviewer has no tools or authority to change merge policy.
- `mimi maintain --status`: inspect saved state without contacting providers.
- `mimi reflect`: consolidate only new user-conversation evidence; preserve live
  sessions. `--force` overrides the evidence gate; `--restart` explicitly opts
  into restarting active bridges. Failed reflection does not advance the cursor.

State lives under `$MIMI_HOME/maintenance` (default `~/.mimi/maintenance`):
`snapshot.json`, `state.json`, `report.md`, `review.json`, `reflect.cursor`, and
release artifacts. Recurring findings keep stable IDs, counts, and first/last-seen
times. A failed collector does not falsely resolve its previous findings. Evidence
includes service health, the entire open PR queue, CI coverage, stale task metadata,
and Telegram access configuration. Reports omit conversation bodies and credentials.
Only finding summaries and PR metadata are sent to the advisory model.

The brief prioritizes up to three evidence-backed issues, matching existing PRs,
unknowns, regression tests, and rollback criteria. It is advisory, not proof that
a proposed change fixes a problem. There is deliberately no unattended code-writing
agent or nightly PR quota. Existing trusted PRs enter evaluation without an owner
label or approval. Mimi requests missing CI, reviews the diff independently, then
tests and releases qualifying repairs. Changes without effective regression tests
remain held; the controller does not invent tests or repair rejected source yet.

## Automatic release policy

Defaults fail closed. To enable, create `~/.mimi/maintenance/config.json`:

```json
{
  "auto_merge": true,
  "trusted_authors": ["YOUR_GITHUB_LOGIN"],
  "merge_label": null
}
```

All of these must hold:

1. Trusted author, same-repository PR, non-draft,
   target `master`, confirmed mergeability, no changes-requested review, and
   successful CI on the exact head revision. Missing/skipped/pending checks block.
   No human review or label is required. `merge_label` may optionally restore an
   opt-in gate for installations that prefer it. Existing changes-requested reviews
   are still respected. For missing CI, the controller dispatches the repository
   workflow on pinned head/base revisions and verifies the resulting run identity.
2. At most six changed files and 400 added/deleted lines. Authentication/channel,
   maintenance, workflow, schema, script, and dependency paths require deliberate
   review outside this automated path. Added destructive SQL migrations also block.
3. A tools-disabled independent model review of the exact diff and full changed
   files must identify a concrete defect and useful regression test, with no
   unresolved risks. Its decision is cached by head/base/diff, cannot override
   policy, and is not a substitute for executed tests. Provider failures retry
   after six hours without permanently quarantining the code. Reviews and CI
   requests are each capped at three per day. Rejected/deferred revisions are held.
   Added regression tests, locked Rust tests/release build, frozen dashboard
   install/build, and scratch-home setup/brain smoke tests must pass against a
   detached worktree combining current master and that exact PR head.
4. Eligibility and master are rechecked after building. Production must have a
   healthy, matching installed binary and a dashboard artifact to restore.
   Candidates must retain the new maintenance/reflection CLI controls; publish
   this baseline in master before any legacy PR can enter the automatic path.
5. Save the previous binary/dashboard and a consistent SQLite backup. Install the
   candidate, restart only previously active services, and require 30 seconds of
   healthy, stable processes running the exact candidate binary before squash merge.
6. Verify GitHub merged the tested tree. Monitor for 24 hours, with at most one
   release per day. The repository's working branch and local edits are untouched.

The trusted-author allowlist is a trust boundary: builds run on this host as its
user, not inside a security sandbox. Automated model review is not a defense
against all malicious code. Do not add untrusted authors or bypass protected paths.

## Recovery and limits

Before mutations the controller journals its phase. The next `--apply` resumes
recovery after a crash. Overlapping invocations and reflection share a file lock.
Build failures never touch production. Deployment/probation failures restore the
previous binary and dashboard, restart and health-check those services, quarantine
the exact failed revision, and impose a cooldown. A failed rollback retains its
journal for retry; further merges stop.

If the PR was merged, source rollback creates a normal forward revert commit using
the prior tree. This is allowed only when the merged commit's sole parent is the
tested base and master still points to that merge. It never force-pushes or
overwrites later work. GitHub outages/protection/racing commits can prevent source
rollback: runtime restoration still happens, the error is persisted, and further
merges stop pending recovery or human review. Retrying an already-applied revert is
idempotent.

Provider outages retain the last brief and deterministic checks. Corrupt state
stops the controller instead of resetting safety gates. It cannot guarantee recovery
from every failure: host/disk failure, lost credentials, external APIs, semantic
answer regressions, and destructive data changes need additional recovery work.
Health checks verify process stability/binary identity and dashboard brain access,
not end-to-end Discord/Telegram replies or answer quality. SQLite snapshots are
**not automatically restored**, because doing so could erase newer user data.
Schema changes need a separate migration/recovery plan; the SQL gate is not a
complete migration detector. Retain required release artifacts and manage disk use;
never delete the active dashboard symlink target or a journal's rollback artifacts.

## Host installation

Requirements: Linux/systemd user services, Python 3.11+, Rust 1.94+, Bun 1.3.11,
Git, authenticated `gh`, Claude CLI for advisory review/reflection, and noninteractive
`sudo install`/`mv` access for `/usr/local/bin/mimi`. Service names and health URL
are configurable through `config.json`; see `DEFAULTS` in `scripts/maintain.py`.

Install the controller independently of the Mimi binary so rollback cannot remove
the recovery logic:

```sh
install -m 755 scripts/maintain.py ~/.local/bin/mimi-maintain
```

Replace the old nightly `mimi audit` and `mimi update` cron entries with a five-minute
`~/.local/bin/mimi-maintain --apply` entry. Preserve the environment needed for
`systemctl --user`, Cargo, Bun, Claude, and `gh`. Keep the nightly evidence-gated
reflection entry. Back up the installed binary and crontab before migrating; align
running services with the new binary once, and verify health before enabling merges.
Installing a standalone copy does not publish the GitHub workflow: the source
changes still need to land in master through deliberate review.

## Useful backlog work

Do not discard all existing proposals: preserve useful fixes while retiring
duplicates only after replacement behavior is verified. Useful starting points:

- #100: backup failure must not kill the dashboard; fix archive exclusions/custom homes.
- #130: SQLite-enforced read-only dashboard queries instead of SQL prefix checks.
- #143: preserve FTS query behavior while handling malformed searches.
- #163 with #58: per-channel memory retention plus cross-process context locking.
- #164/#165: Discord reaction access checks and prevention of duplicate bridges.
- #166: unify CLI/dashboard task storage, with migration and transactional updates.

These are ideas to preserve, not blanket merge approvals. Authentication, memory
isolation, migrations and scheduler ownership need more than compilation. The
multiple scheduler, updater, restart-ping and backup PRs are competing designs;
do not stack them or close alternatives before their useful behavior is covered.

A live independent review of #163 rejected its unchanged revision: its global
ceiling can still evict entire channels, it drops unparseable stored entries, and
it changes retention/privacy behavior without integration coverage. Preserve the
per-channel retention idea, but repair these issues before release.

## Validation

```sh
python3 -m unittest discover -s scripts -p 'test_maintain.py'
cargo test --locked
cargo build --release --locked
cd dashboard && bun install --frozen-lockfile && bun run build
```

Controller tests inject build/health/merge failures and crashes without writing to
GitHub or touching production. GitHub Actions runs the same baseline checks on
master pushes and PRs after this workflow is published.
