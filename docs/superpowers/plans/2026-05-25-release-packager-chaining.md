# Automatic Packager Chaining Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every tagged GitHub Release automatically publish updated Homebrew and Scoop package metadata without a manual workflow dispatch.

**Architecture:** Retain `.github/workflows/bump-packagers.yml` as the only renderer/pusher for external catalogs, but expose it through `workflow_call` with a tag input and the existing catalog write secret. Add a final reusable-workflow job in `.github/workflows/release.yml` that runs only after artifact publication succeeds.

**Tech Stack:** GitHub Actions reusable workflows, YAML, GitHub CLI, Cargo release metadata, Scoop.

---

### Task 1: Make Packager Publishing Callable From Release

**Files:**
- Modify: `.github/workflows/bump-packagers.yml`
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Run a failing structural check for the missing workflow chain**

Run:

```powershell
$bump = Get-Content -Raw '.github/workflows/bump-packagers.yml'
$release = Get-Content -Raw '.github/workflows/release.yml'
if ($bump -notmatch '(?m)^\s{2}workflow_call:') { throw 'missing workflow_call trigger' }
if ($bump -notmatch 'GH_TOKEN:\s+\$\{\{\s*github\.token\s*\}\}') { throw 'asset downloads do not use the source-repo token' }
if ($bump -notmatch 'x-access-token:\$\{PACKAGER_TOKEN\}') { throw 'external catalog pushes do not use PACKAGER_TOKEN' }
if ($release -notmatch 'uses:\s+\./\.github/workflows/bump-packagers\.yml') { throw 'release does not invoke packager workflow' }
```

Expected: FAIL with `missing workflow_call trigger` because the current packager workflow can only run from `release: published` or manual dispatch.

- [ ] **Step 2: Add the reusable interface to `bump-packagers.yml`**

Insert under `on:`:

```yaml
  workflow_call:
    inputs:
      tag:
        description: 'Release tag whose assets should update package catalogs'
        required: true
        type: string
    secrets:
      PACKAGER_TOKEN:
        required: true
```

Set its job environment as:

```yaml
      TAG: ${{ inputs.tag || github.event.release.tag_name }}
      GH_TOKEN: ${{ github.token }}
      PACKAGER_TOKEN: ${{ secrets.PACKAGER_TOKEN }}
```

This accepts an explicit tag for reusable/manual calls while preserving
release-event compatibility. `GH_TOKEN` reads source release assets, including
a manually produced draft; `PACKAGER_TOKEN` remains limited to writes in the
two external catalog repositories. In both clone commands, change the URL to:

```bash
git clone "https://x-access-token:${PACKAGER_TOKEN}@github.com/xom11/homebrew-tap.git" "$tmp"
git clone "https://x-access-token:${PACKAGER_TOKEN}@github.com/xom11/scoop-bucket.git" "$tmp"
```

- [ ] **Step 3: Add the final catalog-publish job to `release.yml`**

After the `release` job, add:

```yaml
  packagers:
    name: Homebrew and Scoop
    needs: release
    uses: ./.github/workflows/bump-packagers.yml
    with:
      tag: ${{ inputs.tag || github.ref_name }}
    secrets: inherit
```

The `needs: release` dependency guarantees catalog rendering only starts after release assets have been uploaded.

- [ ] **Step 4: Run the structural check to verify the workflow chain is present**

Run the PowerShell assertion from Step 1 again.

Expected: exits `0` with no errors.

- [ ] **Step 5: Inspect and commit the CI workflow change**

Run:

```powershell
git diff --check
git diff -- .github/workflows/release.yml .github/workflows/bump-packagers.yml
git add -- .github/workflows/release.yml .github/workflows/bump-packagers.yml
git commit -m "ci(release): chain packager updates after publishing"
```

Expected: a commit adding `workflow_call` and a `packagers` job with no whitespace errors.

### Task 2: Publish A Verification Release

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Bump workspace package versions for the verification release**

Change `version = "0.2.0"` to `version = "0.2.1"` for the workspace package in `Cargo.toml` and the five local `beckon-*` packages in `Cargo.lock`.

- [ ] **Step 2: Verify the release input before committing**

Run:

```powershell
cargo fmt --all -- --check
cargo build --release --locked -p beckon-cli
.\target\release\beckon.exe --version
cargo test --workspace --exclude beckon-linux --exclude beckon-macos
cargo clippy --workspace --exclude beckon-linux --exclude beckon-macos --all-targets -- -D warnings
```

Expected: formatting, build, test, and Clippy commands exit `0`; binary output is `beckon 0.2.1`; Windows tests report `17 passed`.

- [ ] **Step 3: Commit and push source changes, then create the verification tag**

Run:

```powershell
git add -- Cargo.toml Cargo.lock
git commit -m "release: bump version to 0.2.1"
git push origin main
git tag v0.2.1
git push origin v0.2.1
```

Expected: `main` and tag `v0.2.1` are accepted by `origin`.

- [ ] **Step 4: Verify automatic external catalog publishing**

Run:

```powershell
$runs = gh run list --repo xom11/beckon --workflow release.yml --limit 5 --json databaseId,headBranch,status,conclusion | ConvertFrom-Json
$releaseRun = $runs | Where-Object { $_.headBranch -eq 'v0.2.1' } | Select-Object -First 1
gh run watch $releaseRun.databaseId --repo xom11/beckon --exit-status
gh api repos/xom11/scoop-bucket/contents/bucket/beckon.json --jq '.download_url'
gh api repos/xom11/homebrew-tap/contents/Formula/beckon.rb --jq '.download_url'
```

Expected: the `Release` run contains a successful `Homebrew and Scoop` job, and downloaded catalog files contain version `0.2.1` without launching `bump-packagers.yml` via `workflow_dispatch`.

- [ ] **Step 5: Upgrade and smoke-test the Scoop-installed binary**

Run:

```powershell
scoop update
scoop update beckon
beckon --version
beckon -r Settings
beckon -r 'File Explorer'
```

Expected: Scoop installs `0.2.1`, `beckon --version` prints `beckon 0.2.1`, and both Windows targets resolve correctly.
