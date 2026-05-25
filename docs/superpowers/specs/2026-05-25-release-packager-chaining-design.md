# Automatic Packager Chaining Design

**Date:** 2026-05-25
**Scope:** GitHub Actions release automation for Homebrew and Scoop.

## Problem

The `v0.2.0` tag successfully ran `.github/workflows/release.yml` and
published GitHub Release artifacts, but `.github/workflows/bump-packagers.yml`
did not run automatically. The release was published by a workflow using
GitHub's repository token, and GitHub does not create a second workflow run
from that token-generated release event. Updating `homebrew-tap` and
`scoop-bucket` therefore required a manual `workflow_dispatch` run.

## Goal

Pushing a new version tag must produce all release outputs without an
additional manual action:

1. Build and publish release artifacts.
2. Render the Homebrew formula and Scoop manifest from those artifacts.
3. Push the rendered files to `xom11/homebrew-tap` and
   `xom11/scoop-bucket`.

Manual backfill of an existing release must remain available.

## Design

Keep `bump-packagers.yml` as the single implementation of manifest rendering
and catalog pushes, and make it reusable with `workflow_call`. It will accept
a required `tag` input and the existing `PACKAGER_TOKEN` secret when called
from another workflow. Its existing `workflow_dispatch` trigger remains for
backfills, and `release: published` remains compatible with releases
published outside the release workflow.

Extend `release.yml` with a final job after the GitHub Release job:

- It calls `./.github/workflows/bump-packagers.yml`.
- It passes the tag used by the release run, whether the run came from a tag
  push or manual `workflow_dispatch`.
- It inherits repository secrets so the reusable workflow can write to the
  catalog repositories.

Within `bump-packagers.yml`, use the built-in `${{ github.token }}` for
downloading assets from `xom11/beckon`, including draft releases created by
manual release runs. Use `PACKAGER_TOKEN` only in authenticated clone/push
URLs for `homebrew-tap` and `scoop-bucket`. This preserves the existing
restricted PAT scope while making `workflow_dispatch` releases work.

The resulting tag-push path is:

```text
push v* tag -> build artifacts -> publish GitHub Release
            -> call Bump packagers -> push Homebrew/Scoop metadata
```

This avoids depending on event recursion, does not widen token permissions,
and preserves one rendering implementation.

## Version And Verification

The workflow fix will be committed to `main` and verified with a patch release
tag, `v0.2.1`, after updating workspace metadata from `0.2.0` to `0.2.1`.
Evidence required for completion:

- CI and Release workflows for `v0.2.1` finish successfully.
- The packager update appears as part of the release workflow path, without a
  manually dispatched `Bump packagers` run for `v0.2.1`.
- `xom11/scoop-bucket` and `xom11/homebrew-tap` contain version `0.2.1`.
- `scoop update beckon` on the Windows test machine installs `beckon 0.2.1`.

## Non-Goals

- Changing the artifact matrix or packaging file formats.
- Replacing `PACKAGER_TOKEN` or broadening its repository access.
- Removing manual backfill support for already-published releases.
