# Current engineering status

Reviewed 23 October 2026 against default-branch source
`f1f3c27536b5099fd5690b3349d4fefab8eedeb5`, with the workflow/documentation update
in this change. The dated [September review](REVIEW_NOTES.md) and
[checkpoint finding](CRIU_SECURITY.md) describe an earlier baseline; they are
historical evidence rather than the current defect list.

## Changes already merged

| Area | Current source / evidence | Remaining boundary |
| --- | --- | --- |
| Agent listener | Loopback binding plus [endpoint/listener smoke script](../scripts/ci_agent_smoke.sh); [PR 5](https://github.com/omarsaqr12/procsentinel/pull/5) | HTTP remains unauthenticated; GUI/coordinator behavior is a separate check |
| Formatting | Formatting repair in PR 5; [successful historical run](https://github.com/omarsaqr12/procsentinel/actions/runs/35360846114) | Inspect the latest workflow for a new commit |
| Checkpoint IDs and paths | [checkpoint_path](../src/criu_manager.rs) validates 1–128 ASCII letters, digits, underscore or hyphen, canonicalizes the base and rejects symlink/non-directory entries; [PR 4](https://github.com/omarsaqr12/procsentinel/pull/4) | Concurrent replacement/rename races are not fully confined by directory handles |
| Restore result | A unique PID file must be a regular file containing a nonzero PID | This does not prove a real host can checkpoint/restore an arbitrary process |
| Automated checks | [Rust workflow](../.github/workflows/rust-checks.yml): formatting, check, unit tests and loopback agent smoke | Does not validate every UI, process-control or CRIU path |

The final checkpoint repair branch had a
[successful historical run](https://github.com/omarsaqr12/procsentinel/actions/runs/35361686257).
Historical branch success is distinct from a newly executed default-branch run.

## Open engineering work

1. Reject PID 0 and out-of-range PID values consistently, then address PID reuse
   with a process-identity design and disposable-process regression tests.
2. Validate cron expressions before accepting a schedule; malformed input must
   never silently select an every-minute destructive action.
3. Strengthen checkpoint-directory confinement against concurrent renames.
4. Exercise a real CRIU round trip on a controlled Linux host and inspect
   representative terminal/graphical workflows.
5. Add a reproducible benchmark before making quantitative performance claims.

## Repeatable checks

From the repository root on a supported Linux host:

```bash
cargo fmt --all -- --check
cargo check --locked
cargo test --locked
cargo build --locked
bash scripts/ci_agent_smoke.sh
```

The current portfolio review environment has no Rust/Cargo toolchain. Source
inspection and historical CI evidence were available locally; fresh Rust
execution belongs to the linked workflow or a suitable Linux development host.

