# CRIU checkpoint path safety — unresolved, high priority

Source: [`src/criu_manager.rs`](../src/criu_manager.rs), baseline `15dddb6236c8a665644fcbc056a7e3681906041b`. This is a confirmed code-path defect from static inspection; no destructive proof-of-concept was executed.

`checkpoint_process` chooses a caller-provided `checkpoint_id` when present, then constructs `self.checkpoint_base_dir.join(&checkpoint_id)` and creates that directory. `restore_process` and `delete_checkpoint` likewise join the caller's ID. **`delete_checkpoint` calls `std::fs::remove_dir_all(&checkpoint_dir)` without rejecting absolute paths, `..` components, or symlink escapes.** Joining a `PathBuf` with an absolute path discards the prefix, and a relative `../` can escape the intended checkpoint base directory. This is particularly dangerous if the GUI or task configuration passes arbitrary IDs or if the application is run with elevated privileges.

Until corrected, **do not invoke checkpoint deletion with an arbitrary or externally supplied ID, and do not run ProcSentinel as root for normal use.** This branch does not fix the issue; it requires a separate reviewed code change.

Minimum acceptance criteria for a repair:

1. Strictly validate IDs as a single normal path component, with an explicit length limit and an allowlist such as `[A-Za-z0-9_-]`; reject empty strings, `.`, `..`, absolute paths, separators and non-normal components. Validate at every public checkpoint, restore and deletion entry point.
2. Ensure the validated target is a real expected checkpoint directory directly under the canonicalized base, not a symlink; address time-of-check/time-of-use races and avoid blindly following links. Consider an interface based on IDs returned by a trusted checkpoint index rather than arbitrary input.
3. Add temp-directory tests for `../`, absolute paths, symlinks, malformed names, nonexistent checkpoints and a legitimate checkpoint. Tests must never call CRIU or recursively delete anything outside a disposable temp fixture.
4. Review `restore_process`'s `Ok(0)` placeholder after successful CRIU execution; callers must not treat PID 0 as a valid restored process.

The repository's broader [review notes](REVIEW_NOTES.md) and README identify other process-identity and scheduler issues; this finding is additive.
