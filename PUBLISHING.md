# Publishing workflow

Three places hold the work:

| Where | What | Visibility |
|---|---|---|
| live workspace (`~/Desktop/and`, its own git repo) | everything: modules, tools, builder scripts, private results, logs | **private, local only** |
| this repository (`~/Desktop/fp6-nixos`) | curated export: code + docs only | public (once pushed) |
| `~/Desktop/fp6-upstream/<project>/` | upstream clones with `fp6/<topic>` branches, kernel/oFono patch series | pushed to forks / mailed per project |

Work happens in the live workspace. This repository is never edited by hand
except for public-only files (README.md, PUBLISHING.md,
CONTRIBUTING-UPSTREAM.md, LICENSE, `local/README.md`,
`nixos-fp6/assets/README.md`, `tools/publish/`, `tools/fetch-sources.sh`,
`tools/phone-ssh`, `tools/nix-remote`). The export overwrites every other
file it manages (listed in `tools/publish/export-manifest.txt`).

## Every fix: one commit

```sh
cd ~/Desktop/and
tools/publish/sync-and-commit.sh -m "audio: enable the top mic via SWR_MIC0" \
  nixos-fp6/feature-push/audio/ucm/Fairphone/fp6/HiFi.conf \
  nixos-fp6/feature-push/trial-lomiri-full-kernel6-fp6.nix
```

This

1. commits exactly those paths in the private workspace repo (use `--all`
   only when the working tree contains a single change);
2. runs `tools/publish/export-public.sh` (allowlist, text-only, sanitizer,
   prune of files that left the allowlist) and the secret scan, then commits
   the exported change here with the same message, then scans this repo's
   whole history;
3. prints the rows of CONTRIBUTING-UPSTREAM.md that mention the changed
   files, i.e. the `fp6/<topic>` branches or patch series to refresh.

Commit messages: `area: what changed` (areas: device, kernel, audio, camera,
cellular, sensors, power, lomiri, osk, apps, boot-splash, network, misc,
tools, docs). Every commit ends with
`Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (the script adds
it; set `FP6_TRAILER=` for commits made without AI help).

`tools/publish/sync-and-commit.sh --status` shows pending changes in both
repositories. `tools/publish/sync-and-commit.sh --public-only -m ...`
re-exports without a private commit (for example after editing the
allowlist).

### New files

The export takes only allowlisted files. When a target starts importing a
new file outside the allowlisted directories, the export prints
`WARNING: imported by a target but not exported: <path>`. Review the file,
add it to `tools/publish/allowlist.txt` (in this repo), and re-run. To publish
a new build target, add it to `tools/publish/targets.txt`.

### Private inputs of the tools

Kept in the private workspace, never committed (paths can be overridden):

- `FP6_SECRET_PATTERNS` (default
  `~/Desktop/and/results/feature-push-20261024/publish/secret-patterns.txt`):
  literal regexes for the phone password, builder host/user/password, key
  fragments and topics that must never be published. `secret-scan.sh` only
  holds generic patterns (keys, private IP ranges, IMEI/ICCID/EID/IMSI,
  phone numbers, eSIM activation codes, coordinates, Wi-Fi credentials,
  personal paths and e-mail addresses).
- `FP6_EXPORT_DENY` (same directory, `export-deny.txt`): extra deny rules for
  the export.

If the secret scan reports a hit, nothing is committed publicly by
`sync-and-commit.sh` (the export exits non-zero). Fix the source in the live
workspace (move the value to `local/*.env` or an option), or, for a reviewed
false positive, add a narrow regex to `tools/publish/scan-allow.txt`.

If something private did reach a public commit, do not just add a fix-up
commit: rewrite the unpushed history (`git rebase -i` / `git commit --amend`)
before pushing. Once pushed, rotate the secret.

### Machine-specific settings

`local/phone.env` (phone SSH password) and `local/builder.env` (remote ARM
builder store URL) are git-ignored; see `local/README.md`. Tokens are never
stored in any repository.

## Pushing (once credentials exist)

Nothing has been pushed yet. Remotes already configured:

- this repo: `origin` = `https://github.com/URunDEAD/fairphone6-nixos.git`
  (the GitHub repository still has to be created, public, empty, without a
  generated README/license);
- `~/Desktop/fp6-upstream/{mobile-nixos,nixpkgs,maliit-framework,maliit-keyboard,tqftpserv}`:
  `fork` = `https://github.com/URunDEAD/<name>.git` (`origin` is the
  read-only upstream).

### GitHub

The GitHub token lives in `~/Desktop/fp6-work/.github.env` (mode 600,
git-ignored there; never copy it into this repo or the live workspace).
Expected format: a single line containing only the token. If the file uses
`GH_TOKEN=...` instead, strip the prefix when piping it.

```sh
# plain token file
gh auth login --with-token < ~/Desktop/fp6-work/.github.env
# or, if the file reads GH_TOKEN=...
sed -n 's/^GH_TOKEN=//p' ~/Desktop/fp6-work/.github.env | gh auth login --with-token
gh auth setup-git          # lets git use gh for https://github.com remotes
gh auth status
```

The token needs `repo` scope (classic) or, fine-grained, contents
read/write on the target repositories (plus "Administration: write" only to
create the repository with `gh repo create`). Alternatively add an SSH key to
the GitHub account and switch the remotes to
`git@github.com:URunDEAD/<name>.git`.

```sh
# public repo (first push)
gh repo create URunDEAD/fairphone6-nixos --public \
  --description "NixOS (Mobile NixOS + Lomiri/Mir) on the Fairphone 6 - unofficial"
git -C ~/Desktop/fp6-nixos push -u origin main

# upstream branches: push to the fork, then open the PR against upstream
cd ~/Desktop/fp6-upstream/mobile-nixos
git push fork fp6/initrd-zstd
gh pr create --repo mobile-nixos/mobile-nixos --head URunDEAD:fp6/initrd-zstd --fill
```

The nixpkgs clone is sparse and shallow; push branches with
`git push fork fp6/<topic>` (GitHub accepts shallow pushes onto a fork of
the same history). Open nixpkgs PRs against `master` and follow the nixpkgs
PR template.

### GitLab (UBports/Lomiri), GNOME GitLab, Catcrafts Forgejo

Needs accounts on gitlab.com (UBports projects), gitlab.gnome.org (Chatty)
and forgejo.catcrafts.net (imsd, milos-linux). Either `glab auth login`
(token in `~/Desktop/fp6-work/.gitlab.env`, same rules as above:
`glab auth login --stdin < ~/Desktop/fp6-work/.gitlab.env`) or an SSH key.
Fork each project in the web UI, then:

```sh
cd ~/Desktop/fp6-upstream/qtmir
git remote add fork git@gitlab.com:<gitlab-user>/qtmir.git
git push fork fp6/scaled-display-buffer
# open the merge request against ubports/development/core/qtmir (web UI or glab mr create)
```

### Kernel and oFono (mail)

Series are in `~/Desktop/fp6-upstream/linux/series/<topic>/` and
`~/Desktop/fp6-upstream/ofono/series/`. Before sending:

1. Review every patch. For the kernel, regenerate with your sign-off:
   `SIGNOFF=1 ~/Desktop/fp6-upstream/linux/make-series.sh`. **`Signed-off-by`
   certifies the Developer Certificate of Origin
   (https://developercertificate.org); add it only if you agree to it.**
   The patches also carry an `Assisted-by:` tag as the kernel's AI-assistance
   rules ask.
2. Configure `git send-email` (SMTP of your mail provider, an app password
   stored in your system keyring or `~/.git-credentials`, never in a repo).
3. `git send-email --to-cmd=... ` or use the addresses in each series'
   `to-cc.txt`; send to yourself first (`--dry-run`, then `--to` yourself).
4. For milos-linux patches, a pull request on forgejo.catcrafts.net may suit
   the maintainer better than mail.

See `~/Desktop/fp6-upstream/linux/README.md` for details.
