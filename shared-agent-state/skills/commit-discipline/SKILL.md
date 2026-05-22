---
name: commit-discipline
description: The per-task commit/push pattern Lagrange sessions follow. Keeps uncommitted state from accumulating across hours-long work.
---

# Commit and push at task boundaries

Lagrange sessions run for days. The VM can be destroyed at any time.
Any uncommitted change you leave behind disappears on teardown.

## The rule

When you complete a **discrete unit of work**, commit and push *immediately*,
even if the next unit will touch nearby code. A "discrete unit" is anything
you would naturally describe as one thing — a bug fix, a feature
increment, a refactor, a docs update.

If you find yourself thinking "I'll just keep going and commit it all at
the end," stop. That mindset costs you hours when the VM dies.

## Commit message structure

```
<area>: <one-line summary in present tense>

<optional body explaining *why*, not *what*. The diff explains the what.>
```

Examples that are good:

```
runtime: short-circuit ingest loop on empty payload
```

```
deps: bump tokio to 1.43

  Need the new yielding behavior in spawn_blocking to keep p99 latency
  stable under the 10x throughput test.
```

Examples that are bad:

```
fix bug    # what bug?
wip        # never. either it's a step worth keeping or it isn't.
update files   # which? to what?
```

## What "push" means

`git push` to the remote tracked by the branch. If there's no upstream,
set one:

```sh
git push -u origin HEAD
```

If push is rejected (someone else pushed in between), `git pull --rebase`
and try again. Don't force-push to a shared branch.

## When a unit isn't quite ready

If you're mid-refactor and the code doesn't compile, you have two
options that both preserve your work across VM teardown:

1. **Commit anyway with a WIP marker**, push to a non-default branch:

   ```sh
   git checkout -b wip/<short-name>
   git add -A && git commit -m "wip: <what's incomplete>"
   git push -u origin HEAD
   ```

2. **Stash + push as a branch**, less common:

   ```sh
   git stash push -u -m "<context>"
   # later: git stash list / git stash pop
   ```

Either way, the rule is: do not let work-in-progress live only in the VM.

## North star: update the project CLAUDE.md too

When the strategic direction of the work changes (you switched from
"fix the bug" to "rewrite the subsystem"), update the repo's
`CLAUDE.md` in the same commit. The 1M context window will still
compact; the next session needs to know what the current goal is.
