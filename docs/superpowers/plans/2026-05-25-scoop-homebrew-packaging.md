# Scoop bucket + Homebrew tap packaging — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship beckon to end users on macOS/Linux via `brew install xom11/tap/beckon` and on Windows via `scoop install xom11/beckon`, with future releases auto-publishing to both catalogs.

**Architecture:** Single source of truth in `xom11/beckon` (templates + CI workflow). On every `release: published` event, a GitHub Actions job downloads the per-target `.sha256` files from the release, renders the templates, and pushes the resulting Formula + JSON to two newly-created public catalog repos (`xom11/homebrew-tap` and `xom11/scoop-bucket`). The first release is backfilled via `workflow_dispatch`.

**Tech Stack:** GitHub Actions (bash + `gh` CLI), Homebrew Ruby DSL, Scoop JSON schema, fine-grained PAT for cross-repo write.

**Spec:** `docs/superpowers/specs/2026-05-25-scoop-homebrew-packaging-design.md`

---

### Task 1: Create Homebrew Formula template

**Files:**
- Create: `packaging/homebrew/beckon.rb.template`

- [ ] **Step 1: Write the template**

```ruby
class Beckon < Formula
  desc "Cross-platform focus-or-launch app switcher"
  homepage "https://github.com/xom11/beckon"
  version "{{VERSION}}"
  license any_of: ["Apache-2.0", "MIT"]

  on_macos do
    on_arm do
      url "https://github.com/xom11/beckon/releases/download/v#{version}/beckon-#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "{{SHA256_DARWIN_ARM}}"
    end
    on_intel do
      url "https://github.com/xom11/beckon/releases/download/v#{version}/beckon-#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "{{SHA256_DARWIN_X86}}"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/xom11/beckon/releases/download/v#{version}/beckon-#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "{{SHA256_LINUX_ARM}}"
    end
    on_intel do
      url "https://github.com/xom11/beckon/releases/download/v#{version}/beckon-#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "{{SHA256_LINUX_X86}}"
    end
  end

  def install
    bin.install "beckon"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/beckon --version")
  end
end
```

---

### Task 2: Create Scoop manifest template

**Files:**
- Create: `packaging/scoop/beckon.json.template`

- [ ] **Step 1: Write the template**

```json
{
  "version": "{{VERSION}}",
  "description": "Cross-platform focus-or-launch app switcher",
  "homepage": "https://github.com/xom11/beckon",
  "license": "Apache-2.0 OR MIT",
  "architecture": {
    "64bit": {
      "url": "https://github.com/xom11/beckon/releases/download/v{{VERSION}}/beckon-{{VERSION}}-x86_64-pc-windows-msvc.zip",
      "hash": "{{SHA256_WIN_X86}}"
    },
    "arm64": {
      "url": "https://github.com/xom11/beckon/releases/download/v{{VERSION}}/beckon-{{VERSION}}-aarch64-pc-windows-msvc.zip",
      "hash": "{{SHA256_WIN_ARM}}"
    }
  },
  "bin": "beckon.exe",
  "checkver": "github",
  "autoupdate": {
    "architecture": {
      "64bit": {
        "url": "https://github.com/xom11/beckon/releases/download/v$version/beckon-$version-x86_64-pc-windows-msvc.zip"
      },
      "arm64": {
        "url": "https://github.com/xom11/beckon/releases/download/v$version/beckon-$version-aarch64-pc-windows-msvc.zip"
      }
    },
    "hash": {
      "url": "$url.sha256"
    }
  }
}
```

---

### Task 3: Smoke-test both templates render correctly

Rendering with sample values catches typos in placeholder names and gross syntax errors before they hit a real release.

**Files:**
- (no new files; throwaway render to `/tmp`)

- [ ] **Step 1: Render Formula with placeholder values**

```bash
sed \
  -e 's|{{VERSION}}|0.1.0|g' \
  -e 's|{{SHA256_DARWIN_ARM}}|aaaa000000000000000000000000000000000000000000000000000000000001|g' \
  -e 's|{{SHA256_DARWIN_X86}}|aaaa000000000000000000000000000000000000000000000000000000000002|g' \
  -e 's|{{SHA256_LINUX_ARM}}|aaaa000000000000000000000000000000000000000000000000000000000003|g' \
  -e 's|{{SHA256_LINUX_X86}}|aaaa000000000000000000000000000000000000000000000000000000000004|g' \
  packaging/homebrew/beckon.rb.template > /tmp/beckon.rb
ruby -c /tmp/beckon.rb
```

Expected: `Syntax OK`. (No remaining `{{...}}` markers — verify with `! grep '{{' /tmp/beckon.rb`.)

- [ ] **Step 2: Render Scoop manifest with placeholder values**

```bash
sed \
  -e 's|{{VERSION}}|0.1.0|g' \
  -e 's|{{SHA256_WIN_X86}}|aaaa000000000000000000000000000000000000000000000000000000000005|g' \
  -e 's|{{SHA256_WIN_ARM}}|aaaa000000000000000000000000000000000000000000000000000000000006|g' \
  packaging/scoop/beckon.json.template > /tmp/beckon.json
python3 -m json.tool /tmp/beckon.json > /dev/null
```

Expected: no output (success). Confirm no leftover placeholders: `! grep '{{' /tmp/beckon.json`.

- [ ] **Step 3: Cleanup**

```bash
rm /tmp/beckon.rb /tmp/beckon.json
```

---

### Task 4: Create the bump-packagers workflow

**Files:**
- Create: `.github/workflows/bump-packagers.yml`

- [ ] **Step 1: Write the workflow YAML**

```yaml
name: Bump packagers
on:
  release:
    types: [published]
  workflow_dispatch:
    inputs:
      tag:
        description: 'Existing release tag to backfill (e.g. v0.1.0)'
        required: true
        type: string

permissions:
  contents: read

jobs:
  bump:
    runs-on: ubuntu-latest
    env:
      TAG: ${{ inputs.tag || github.event.release.tag_name }}
      GH_TOKEN: ${{ secrets.PACKAGER_TOKEN }}
    steps:
      - uses: actions/checkout@v6

      - name: Resolve version + fetch sha256 files
        id: hashes
        run: |
          set -euo pipefail
          version="${TAG#v}"
          echo "version=${version}" >> "$GITHUB_OUTPUT"
          mkdir -p _hashes
          for tgt in \
            aarch64-apple-darwin \
            x86_64-apple-darwin \
            aarch64-unknown-linux-gnu \
            x86_64-unknown-linux-gnu \
            aarch64-pc-windows-msvc \
            x86_64-pc-windows-msvc
          do
            ext=tar.gz
            case "$tgt" in *windows*) ext=zip ;; esac
            asset="beckon-${version}-${tgt}.${ext}.sha256"
            gh release download "${TAG}" -R xom11/beckon -p "$asset" -D _hashes
            hash="$(awk '{print $1}' "_hashes/${asset}")"
            key="$(echo "$tgt" | tr 'a-z-' 'A-Z_')"
            echo "SHA_${key}=${hash}" >> "$GITHUB_OUTPUT"
          done

      - name: Render Formula
        run: |
          set -euo pipefail
          v='${{ steps.hashes.outputs.version }}'
          sed \
            -e "s|{{VERSION}}|${v}|g" \
            -e "s|{{SHA256_DARWIN_ARM}}|${{ steps.hashes.outputs.SHA_AARCH64_APPLE_DARWIN }}|g" \
            -e "s|{{SHA256_DARWIN_X86}}|${{ steps.hashes.outputs.SHA_X86_64_APPLE_DARWIN }}|g" \
            -e "s|{{SHA256_LINUX_ARM}}|${{ steps.hashes.outputs.SHA_AARCH64_UNKNOWN_LINUX_GNU }}|g" \
            -e "s|{{SHA256_LINUX_X86}}|${{ steps.hashes.outputs.SHA_X86_64_UNKNOWN_LINUX_GNU }}|g" \
            packaging/homebrew/beckon.rb.template > beckon.rb

      - name: Render Scoop manifest
        run: |
          set -euo pipefail
          v='${{ steps.hashes.outputs.version }}'
          sed \
            -e "s|{{VERSION}}|${v}|g" \
            -e "s|{{SHA256_WIN_X86}}|${{ steps.hashes.outputs.SHA_X86_64_PC_WINDOWS_MSVC }}|g" \
            -e "s|{{SHA256_WIN_ARM}}|${{ steps.hashes.outputs.SHA_AARCH64_PC_WINDOWS_MSVC }}|g" \
            packaging/scoop/beckon.json.template > beckon.json

      - name: Push Formula to homebrew-tap
        run: |
          set -euo pipefail
          v='${{ steps.hashes.outputs.version }}'
          tmp="$(mktemp -d)"
          git clone "https://x-access-token:${GH_TOKEN}@github.com/xom11/homebrew-tap.git" "$tmp"
          mkdir -p "$tmp/Formula"
          mv beckon.rb "$tmp/Formula/beckon.rb"
          cd "$tmp"
          git config user.name  'beckon-release-bot'
          git config user.email 'beckon-release-bot@users.noreply.github.com'
          git add Formula/beckon.rb
          if git diff --staged --quiet; then
            echo "No changes to push"
            exit 0
          fi
          git commit -m "beckon ${v}"
          git push

      - name: Push manifest to scoop-bucket
        run: |
          set -euo pipefail
          v='${{ steps.hashes.outputs.version }}'
          tmp="$(mktemp -d)"
          git clone "https://x-access-token:${GH_TOKEN}@github.com/xom11/scoop-bucket.git" "$tmp"
          mkdir -p "$tmp/bucket"
          mv beckon.json "$tmp/bucket/beckon.json"
          cd "$tmp"
          git config user.name  'beckon-release-bot'
          git config user.email 'beckon-release-bot@users.noreply.github.com'
          git add bucket/beckon.json
          if git diff --staged --quiet; then
            echo "No changes to push"
            exit 0
          fi
          git commit -m "beckon ${v}"
          git push
```

- [ ] **Step 2: Sanity-check YAML syntax**

```bash
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/bump-packagers.yml'))"
```

Expected: no output (success). If `yaml` is missing locally, skip — GitHub will validate on push.

---

### Task 5: Add Installation section to README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Inspect current README to find the right insertion point**

Read the first ~60 lines of `README.md` to find an appropriate location (after the title/intro, before build-from-source instructions). The Installation section should come BEFORE existing "Build" / "Cargo" content.

- [ ] **Step 2: Insert Installation section**

Use the Edit tool to add this block at the chosen location. Pick an existing heading (e.g. just after the description paragraph) and insert above it.

```md
## Installation

### macOS / Linux (Homebrew)

```sh
brew install xom11/tap/beckon
```

### Windows (Scoop)

```sh
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install xom11/beckon
```

### From source (any platform)

```sh
cargo install --git https://github.com/xom11/beckon beckon-cli
```

Requires `rustup` plus a system C/MSVC toolchain (Xcode CLI tools on macOS, build-essential on Linux, VS Build Tools on Windows).

### Nix

```sh
nix run github:xom11/beckon -- -l
```

Or wire `inputs.beckon.overlays.default` into your nixpkgs to expose `pkgs.beckon`.
```

---

### Task 6: Update CLAUDE.md Distribution section

**Files:**
- Modify: `CLAUDE.md` (the `## Distribution` section)

- [ ] **Step 1: Replace the existing Distribution section**

The current section starts with `## Distribution` and contains GitHub / Cargo / Nix entries only. Use Edit to expand the opening bullets (preserving the rest of the section about flake-input wiring) to:

```md
## Distribution

- **GitHub**: https://github.com/xom11/beckon (source + tagged release artifacts)
- **Homebrew tap** (macOS / Linux): `brew install xom11/tap/beckon` — tap repo `xom11/homebrew-tap`, formula auto-bumped by `.github/workflows/bump-packagers.yml` on every release
- **Scoop bucket** (Windows): `scoop bucket add xom11 https://github.com/xom11/scoop-bucket && scoop install xom11/beckon` — bucket repo `xom11/scoop-bucket`, manifest auto-bumped by the same workflow
- **Cargo (from git)**: `cargo install --git https://github.com/xom11/beckon beckon-cli`
- **Nix flake**: `nix run github:xom11/beckon -- -l` or pull `inputs.beckon.overlays.default` into your nixpkgs.
```

Keep everything from the original section about flake-input wiring, sway integration, Hammerspoon, etc. unchanged. Only the opening bullets are being expanded.

---

### Task 7: Commit + push all source-side changes

**Files:**
- All from Tasks 1, 2, 4, 5, 6.

- [ ] **Step 1: Stage**

```bash
git add packaging/homebrew/beckon.rb.template \
        packaging/scoop/beckon.json.template \
        .github/workflows/bump-packagers.yml \
        README.md \
        CLAUDE.md
git status
```

Expected: 5 files (3 new, 2 modified). Nothing else staged.

- [ ] **Step 2: Commit**

```bash
git commit -m "packaging: scoop bucket + homebrew tap auto-publish

Templates in packaging/{homebrew,scoop}/ are the canonical source.
bump-packagers.yml runs on release: published, downloads the per-target
.sha256 files, renders the templates, and pushes to xom11/homebrew-tap
(Formula/beckon.rb) and xom11/scoop-bucket (bucket/beckon.json).

Backfilling v0.1.0 via workflow_dispatch once PACKAGER_TOKEN is set."
```

- [ ] **Step 3: Push to main**

```bash
git push origin main
```

Expected: push succeeds. `xom11/beckon` main now contains templates + workflow.

---

### Task 8: Create xom11/homebrew-tap repo

**Files:**
- External: create new repo `xom11/homebrew-tap`

- [ ] **Step 1: Create the repo via `gh`**

```bash
gh repo create xom11/homebrew-tap \
  --public \
  --description "Homebrew tap for xom11 tools (beckon, …)" \
  --license MIT
```

Expected: `✓ Created repository xom11/homebrew-tap on GitHub`. (Adding `--license MIT` initializes the repo with a LICENSE file, which is convenient — the actual Formula will be appended later.)

- [ ] **Step 2: Seed with a README**

```bash
tmp="$(mktemp -d)"
gh repo clone xom11/homebrew-tap "$tmp"
cat > "$tmp/README.md" <<'EOF'
# xom11 Homebrew tap

Tap for [xom11](https://github.com/xom11) tools.

## Usage

```sh
brew tap xom11/tap
brew install beckon
# or, without tapping first:
brew install xom11/tap/beckon
```

## Available formulae

- [beckon](https://github.com/xom11/beckon) — cross-platform focus-or-launch app switcher

## Auto-generated

This repo is updated automatically by the
[`bump-packagers.yml`](https://github.com/xom11/beckon/blob/main/.github/workflows/bump-packagers.yml)
workflow in `xom11/beckon` on every release. Do not hand-edit `Formula/*.rb`
— changes will be overwritten by the next release.
EOF
cd "$tmp"
git add README.md
git commit -m "docs: initial README"
git push
cd -
rm -rf "$tmp"
```

Expected: README pushed. Visiting https://github.com/xom11/homebrew-tap shows it.

---

### Task 9: Create xom11/scoop-bucket repo

**Files:**
- External: create new repo `xom11/scoop-bucket`

- [ ] **Step 1: Create the repo via `gh`**

```bash
gh repo create xom11/scoop-bucket \
  --public \
  --description "Scoop bucket for xom11 tools (beckon, …)" \
  --license MIT
```

Expected: `✓ Created repository xom11/scoop-bucket on GitHub`.

- [ ] **Step 2: Seed with a README**

```bash
tmp="$(mktemp -d)"
gh repo clone xom11/scoop-bucket "$tmp"
cat > "$tmp/README.md" <<'EOF'
# xom11 Scoop bucket

Bucket for [xom11](https://github.com/xom11) tools.

## Usage

```powershell
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install beckon
# or:
scoop install xom11/beckon
```

## Available manifests

- [beckon](https://github.com/xom11/beckon) — cross-platform focus-or-launch app switcher (x86_64 + arm64)

## Auto-generated

This repo is updated automatically by the
[`bump-packagers.yml`](https://github.com/xom11/beckon/blob/main/.github/workflows/bump-packagers.yml)
workflow in `xom11/beckon` on every release. Do not hand-edit `bucket/*.json`
— changes will be overwritten by the next release.
EOF
cd "$tmp"
git add README.md
git commit -m "docs: initial README"
git push
cd -
rm -rf "$tmp"
```

Expected: README pushed. Visiting https://github.com/xom11/scoop-bucket shows it.

---

### Task 10: USER ACTION — create PAT and add as secret

This step **cannot be done by Claude**. The user creates a personal access token in their GitHub account and stores it as a repo secret on `xom11/beckon`. Claude pauses here until the user confirms completion.

- [ ] **Step 1: User creates a fine-grained PAT**

1. Open https://github.com/settings/personal-access-tokens/new (or Settings → Developer Settings → Personal access tokens → Fine-grained tokens → Generate new token)
2. **Token name**: `beckon-packagers-bump`
3. **Resource owner**: xom11
4. **Expiration**: 90 days (or longer; renewal procedure documented in tap README)
5. **Repository access**: "Only select repositories" → select `xom11/homebrew-tap` AND `xom11/scoop-bucket`. **Do NOT include** `xom11/beckon`.
6. **Repository permissions**: set **Contents** to **Read and write**. Leave everything else as **No access**.
7. Click **Generate token**. Copy the token immediately (only shown once).

- [ ] **Step 2: User adds the token as a repo secret on xom11/beckon**

```bash
gh secret set PACKAGER_TOKEN -R xom11/beckon
# Paste the token when prompted, press Enter.
```

Alternative (web UI): https://github.com/xom11/beckon/settings/secrets/actions → New repository secret → Name: `PACKAGER_TOKEN`, Value: (paste).

- [ ] **Step 3: User confirms to Claude**

User replies "done" / "tiếp tục" / equivalent. Claude moves to Task 11.

---

### Task 11: Backfill v0.1.0 via workflow_dispatch

**Files:**
- External: `xom11/beckon` Actions tab → `Bump packagers` workflow

- [ ] **Step 1: Dispatch the workflow**

```bash
gh workflow run bump-packagers.yml -R xom11/beckon -f tag=v0.1.0
```

Expected: `✓ Created workflow_dispatch event for bump-packagers.yml`.

- [ ] **Step 2: Watch the run**

```bash
sleep 3
gh run list -R xom11/beckon --workflow=bump-packagers.yml --limit=1
run_id="$(gh run list -R xom11/beckon --workflow=bump-packagers.yml --limit=1 --json databaseId --jq '.[0].databaseId')"
gh run watch "$run_id" -R xom11/beckon
```

Expected: all 5 steps green (Checkout, Resolve+fetch hashes, Render Formula, Render Scoop manifest, Push to homebrew-tap, Push to scoop-bucket). Total runtime ~30–60 seconds.

If the run fails on "Push to ..." with HTTP 403, the PAT scope is wrong — re-check Task 10 step 1 settings.

- [ ] **Step 3: Verify Formula committed to homebrew-tap**

```bash
gh api repos/xom11/homebrew-tap/contents/Formula/beckon.rb --jq '.size, .download_url'
gh api repos/xom11/homebrew-tap/contents/Formula/beckon.rb --jq '.content' \
  | base64 -d \
  | grep -E "^  version|sha256" | head -8
```

Expected: file exists, version is `0.1.0`, sha256 lines show real hashes (not placeholders).

- [ ] **Step 4: Verify manifest committed to scoop-bucket**

```bash
gh api repos/xom11/scoop-bucket/contents/bucket/beckon.json --jq '.size, .download_url'
gh api repos/xom11/scoop-bucket/contents/bucket/beckon.json --jq '.content' \
  | base64 -d \
  | python3 -m json.tool \
  | grep -E '"version"|"hash"'
```

Expected: file exists, version is `0.1.0`, both `hash` fields are real 64-char hex strings.

---

### Task 12: USER VERIFICATION — Homebrew install on macOS

User runs this on a macOS machine. Claude cannot perform this; results are reported back.

- [ ] **Step 1: User tries the install**

```bash
brew untap xom11/tap 2>/dev/null   # in case of stale cache
brew install xom11/tap/beckon
beckon --version
```

Expected:
- `brew install` downloads the platform-correct tarball, runs the `test do` block (which calls `beckon --version`), and reports success.
- `beckon --version` prints `beckon 0.1.0` (or whatever the bin reports).

If the install fails with "SHA256 mismatch": the workflow rendered wrong hashes. Re-check Task 11 step 3/4 output against the .sha256 files in the GitHub release.

- [ ] **Step 2: User reports back**

Pass / fail with output.

---

### Task 13: USER VERIFICATION — Scoop install on Windows

User runs this on a Windows machine. Claude cannot perform this; results are reported back.

- [ ] **Step 1: User tries the install**

```powershell
scoop bucket add xom11 https://github.com/xom11/scoop-bucket
scoop install xom11/beckon
beckon --version
```

Expected:
- `scoop install` downloads the architecture-correct zip (x86_64 or arm64), extracts `beckon.exe` into the scoop apps dir, adds it to PATH (via shim).
- `beckon --version` prints `beckon 0.1.0`.

If the install fails with "hash check failed": same diagnosis as Task 12.

- [ ] **Step 2: User reports back**

Pass / fail with output.

---

## Self-review

Spec coverage:
- ✅ Goal — Tasks 1–13 collectively
- ✅ Architecture diagram — Tasks 1, 2, 4 (in-repo files) + Tasks 8, 9 (catalog repos)
- ✅ Manifest contents — Tasks 1, 2
- ✅ CI workflow — Task 4
- ✅ Repo changes in xom11/beckon — Tasks 1, 2, 4, 5, 6 then commit in Task 7
- ✅ Bootstrap 8 steps — Tasks 7 (step 3), 8 (step 1), 9 (step 2), 10 (steps 4–5), 11 (step 6), 12 (step 7), 13 (step 8)
- ✅ Verification — Tasks 11–13
- ✅ Idempotency — Workflow's `git diff --staged --quiet` short-circuit in Task 4

Placeholder scan: no TBD / TODO / "implement later". All code blocks contain real content.

Type consistency: template placeholder names match between Tasks 1, 2 and the sed expressions in Tasks 3, 4. `{{VERSION}}`, `{{SHA256_DARWIN_ARM}}`, `{{SHA256_DARWIN_X86}}`, `{{SHA256_LINUX_ARM}}`, `{{SHA256_LINUX_X86}}`, `{{SHA256_WIN_X86}}`, `{{SHA256_WIN_ARM}}` — 7 distinct keys, each used exactly once in templates and once in sed.

Workflow output keys: `SHA_AARCH64_APPLE_DARWIN`, `SHA_X86_64_APPLE_DARWIN`, `SHA_AARCH64_UNKNOWN_LINUX_GNU`, `SHA_X86_64_UNKNOWN_LINUX_GNU`, `SHA_AARCH64_PC_WINDOWS_MSVC`, `SHA_X86_64_PC_WINDOWS_MSVC` — derived from `tr 'a-z-' 'A-Z_'` over the target triples, matches what the sed expressions reference.

No gaps.
