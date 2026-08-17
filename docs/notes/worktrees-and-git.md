# Worktrees, and the git traps that made them a rule

Extracted from `CLAUDE.md` 2026-08-17. The rule and the checklist live there;
this file holds the measurements behind them. Nothing here was deleted — if a
sentence contradicts CLAUDE.md, this file is the older one and CLAUDE.md wins.

## Why one worktree per session is a rule and not a preference

Measured on 2026-08-14: the primary checkout held **970 uncommitted lines
belonging to two unrelated workstreams at once** — a Hyprland-parity change
(`hyprland.rs`, `CLAUDE.md`, `testing/linux_live_test.py`) and a
`check --resolve` implementation (`beckon-cli/src/lib.rs`, two test files) —
while `ListAgents` showed three Claude sessions with that directory open. The
failure modes are not hypothetical and not visible from inside any one session:

- `git status` cannot say which change belongs to whom, so nobody can commit
  without either sweeping in a stranger's work or hand-picking hunks.
- **`git switch` in one session silently re-homes every commit another session
  makes next.** One did, mid-edit. The victim is not warned at any point, and
  the obvious defence does not exist. Committing to `main` is not an error, so
  git has nothing to say: the most it would print is the ordinary
  `[main abc1234] …` — and that line was not printed either, because the
  commits went through `git commit -q`, which suppresses it. The check that
  followed looked conclusive and was not: **`git log --oneline -1` never names
  the branch you are on.** Five commits landed on `main` while every command
  involved reported success. This is why the rule is *run
  `git branch --show-current`*, not *read the output more carefully* — on this
  path there is no output to read.
  - And the push that should have caught it does not. `git push -u origin
    <branch>` pushed that untouched branch and printed `remote: Create a pull
    request for '<branch>' …` — **an empty push and a real one print the same
    thing**, and `-q` does not suppress it because it is a remote message. It
    surfaced only when `gh pr create` refused with *"you must first push the
    current branch"*, which is a different tool, much later. Verify instead of
    reading output: `git branch --show-current` **before** committing, and
    `git branch -vv` (it prints `[origin/<branch>: ahead N]`) or
    `git ls-remote --heads origin <branch>` against your local SHA **after**
    pushing.
- A `CLAUDE.md` edit from either session lands on top of the other's
  uncommitted text and is swept into whichever commit is made first.
- **Two sessions independently designed the *same* flag with *opposite*
  semantics** — `--resolve` exiting non-zero versus never changing the exit
  code. Note what did *not* cause this: one side's design was **committed**, on
  its own branch, the whole time. It was invisible because nobody fetched or
  listed branches, not because it was unwritten.

**So: a worktree prevents two sessions colliding on a FILE. It does nothing
about two sessions building the same THING.** Two spotless worktrees produce
the duplicate-design failure just as readily. Only the "look for company" rule
catches that one.

## The shared `target/` directory

Share it for `check`, `clippy` and `fmt`. Do not trust it for `build` and
`run`. `target/` is ~7.4 GB and this workspace also cross-compiles to
`aarch64-pc-windows-msvc`, so a fresh worktree rebuilds all of it: export
`CARGO_TARGET_DIR=~/Documents/dev/beckon/target` and take the saving. Cargo
locks the directory, so concurrent builds serialise rather than interleave —
but "they do not corrupt each other" is where the useful half of that sentence
stops. Three failure modes, all measured 2026-08-15 with several worktrees
live, in ascending order of how long they cost:

1. **A stale rlib produces a compile error naming a symbol that is plainly in
   your source.** Ours said `no variant named 'Reset' for enum DefaultButton`
   about a file the task had never touched, while `Reset` sat in the enum three
   lines from where `grep` found it. **Rule: an error about code you can grep
   is a stale artifact, not a bug.**
2. **`cargo clean -p <pkg>` does not clean cross-target artifacts**, so the
   obvious fix appears to disprove the diagnosis: the clean runs, removes
   ~99 MB, and the build fails *identically*, which reads as "so it is not the
   cache" and sends you hunting a real bug. Pass the flag —
   `cargo clean -p beckon-core -p beckon-windows --target
   aarch64-pc-windows-msvc` removed a further 882 MB and the check then passed
   in 0.8 s.
3. **`target/debug/beckon` is one path shared by every worktree, so the binary
   you run may be another branch's** — and this one reports nothing at all.
   `cargo build` said `Finished in 0.08s` and the binary at that path had no
   `--resolve` flag, i.e. it predated `origin/main`. There is no error to
   notice; you simply measure the wrong program. **Build into a private
   `CARGO_TARGET_DIR` whenever you intend to run the binary and believe its
   output.**

Unrelated to worktrees but it compounds all three: **the first exec of a
freshly linked binary is killed on this machine** (exit 137, empty output), and
the second succeeds. It makes a fresh `--help | grep` return nothing and a
fresh test binary report a failure, neither of which is true. Re-run before
believing either.

## Removing a worktree

**REFUTED 2026-08-17.** The entry used to read: *"A session cannot remove its
own worktree: git refuses while the branch is checked out there, and a Claude
session is `cd`'d inside it. So the last session on a branch … leaves the
worktree and the local branch for someone standing in the primary checkout."*
Two sessions independently removed their own worktrees that day, which prompted
a probe, and the probe refuted the *replacement* explanation too — both
sessions had concluded "the shell must not be inside it", and that is also
wrong. Four cases, one temporary worktree each:

```text
worktree remove, shell INSIDE the worktree, clean   exit 0, no output
worktree remove, shell in the primary checkout      exit 0, no output
worktree remove, tree has a modified file           fatal: contains modified
                                                    or untracked files, use --force
git branch -D <b>, while a worktree holds <b>       error: cannot delete branch
                                                    'b' used by worktree at …
```

So **`git worktree remove` never cared who you are or where you stand.** The
original sentence conflated two commands: the refusal it describes is real and
belongs to `git branch -d`. Hence the order in the cleanup line is load-bearing,
not stylistic — **remove the worktree first, then delete the branch**, because
the branch cannot go while any worktree holds it. Doing it the other way round
produces exactly the error that started this entry.

The one real gate is `--force`, and it is worth pausing on rather than
reflexively passing: a worktree with modified or untracked files is somebody
mid-task, or your own unsaved probe. Look at what
`git -C <worktree> status --porcelain` prints before forcing.

**A session still cannot delete a branch that its own worktree holds** — that
much survives. The escape is simply to remove the worktree first, from
anywhere, which the session can do itself.

**`.claude/worktrees/` is a second home for these.** Claude Code makes its own
worktrees there rather than in `.worktrees/`, so `git worktree list` shows both
shapes and a cleanup that only looks at `.worktrees/` misses half the
inventory. `four-doors-phase-0` lived at `.claude/worktrees/four-doors-phase-0`
for its whole life and was recorded as a "stray" for being in the wrong
directory, when it was simply made by a different tool.

## `git branch -vv`: which side is stale

Added 2026-08-16, after the checklist sent a session to the wrong conclusion.
It ran every other line, saw a branch called `four-doors-phase-0` it did not
recognise, and measured it with
`git rev-list --left-right --count main...four-doors-phase-0`, which returned
`0 55`. It read that as *"an unmerged branch, 55 commits ahead of main"* and
told the user that the settings-window design was about to be replaced. **The
truth was the mirror image: the branch was already merged, and the primary
checkout's local `main` was 55 commits behind.** `origin/main`,
`four-doors-phase-0` and the `v0.9.4` tag were all one commit.

The trap is that `git fetch --all` *had* been run, and it does exactly what it
says: it updates `origin/main` and does **not** touch `main`. Every command in
the old list reads a ref, and none of them compares a local branch to its
upstream — so **the same number supports both readings and nothing in that
output distinguishes them.** That is why the fix is a different command rather
than closer reading: `git branch -vv` prints `[origin/main: behind 55]` and
says which side is stale.

It is the same shape as the push trap above — an empty push and a real one
print the same line — and it has the same escape: ask git for the state, do not
squint at output that does not carry it.

## `git cherry`: counting refs is not measuring content

Added 2026-08-17, when a session about to cut a release listed four unmerged
branches and two of them were already in `main` in full. GitHub's *Rebase and
merge* — the button this repo uses, because its history is linear — replays
each commit onto a new parent, so the SHA changes while the patch does not.
Every ref-counting command then reports work that is not missing. Measured on
one temporary branch, its commit cherry-picked onto a newer `main` to reproduce
exactly what the button does:

```text
git rev-list --count base..feature          1        "one commit missing"
git rev-list --left-right --count base...   4/1      same claim, two columns
git cherry base feature                     - d998fe2   '-' = patch IS in base
git diff --stat base feature                (only what base gained since)
```

**`git cherry` compares patch-ids, so it answers the question actually being
asked**, and `git diff --stat origin/main <branch>` answers it a second way —
though read its direction carefully: it also shows everything `main` gained
*after* the branch, which reads alarmingly like unmerged work and is not.

Two consequences, both of which cost time on the day:

- **Before deleting a branch, check with `git cherry`, never a count.** A count
  of 5 on a branch whose five patches are all `-` is the safe case, and it
  looks exactly like the dangerous one.
- **Stale remote-tracking refs inflate the same list.** `git push origin
  --delete <b>` and a `gh pr merge --delete-branch` remove the branch on the
  server; every *other* clone keeps `origin/<b>` until someone runs
  `git fetch --prune`. Two of the four "unmerged branches" above were refs to
  branches that no longer existed. `git ls-remote --heads origin` asks the
  server and cannot be stale.

Uncommitted work in the shared checkout means somebody is mid-task. A *branch*
carrying a spec or a design doc means somebody has already decided something —
and other people's work is far more often committed-but-unmerged than
uncommitted. Either way the answer is the same: reconcile the plan before
executing it, and talk to the other session if there is one.

## `git commit --only`

Measured 2026-08-16 in `~/.nix`: several Claude sessions share that repo, and
`git commit` takes the whole INDEX, not the file just added — a peer staged
three files between the `git status` read and the commit, and 110 lines of
their work landed inside a commit whose message was about bumping a flake pin.
`--only` ignores the index entirely, so a race cannot widen the commit; verify
with `git show --stat HEAD` rather than trusting the commit summary.
