# Fork CI and distribution

This fork (`puxu-msft/herdr`) tracks upstream [herdrdev/herdr](https://github.com/herdrdev/herdr) but replaces its GitHub automation and publishes its own builds. Upstream release instructions in `AGENTS.md` (`just preview`, `just release*`, preview-to-stable promotion, `herdr.dev` manifests, website deploys) describe upstream only and do not work here.

## One-time repository setup

- Enable Actions in the repository's Actions tab.
- No secrets or variables are required; every workflow uses `GITHUB_TOKEN`.
- Leave fork releases as the repository's latest release. Fork builds read their update manifest from `releases/latest/download/preview.json`, so marking another release as latest breaks updates.

## Workflows

| Workflow | Trigger | Purpose |
|---|---|---|
| `fork-ci.yml` | Push to any branch, pull requests from other repositories, manual | Commit subject check (merge commits exempt) and `fork-checks.yml` |
| `fork-checks.yml` | Called by CI and releases | Lint and tests on Linux, macOS, Windows x64, and Windows ARM64; Windows ConPTY packaging for both architectures |
| `fork-windows-installer.yml` | Changes to `distribution/install.ps1` or its ARM64 test, manual | Windows ARM64 install against the upstream preview manifest (x86_64 fallback while upstream has no ARM64 asset) |
| `fork-build.yml` | Called by releases, manual | Release builds of the five assets; manual runs only upload workflow artifacts |
| `fork-release.yml` | Push of a `fork-*` tag, manual run on `master` | Checks and builds, GitHub release publishing, install checks on every native platform |

Shared toolchain setup lives in `.github/actions/fork-setup`. Rust follows `rust-toolchain.toml`; the Zig and Bun versions and pinned action revisions are set in that action and the fork workflows.
Windows ARM64 runners use the x64 Zig 0.16.0 executable under Windows emulation to cross-compile the ARM64 target because native Zig 0.16.0 crashes on those runners. The setup action checks its archive against a pinned SHA-256 digest.

## Publishing a build

Push the commit, then run `scripts/fork/publish.sh [ref]` (default `HEAD`). It creates and pushes the tag `fork-<commit date>-<sha12>`, which starts `fork-release.yml`. On `master` you can instead run the Fork release workflow from the Actions tab.

Each release contains the five platform assets (`herdr-linux-x86_64`, `herdr-linux-aarch64`, `herdr-macos-aarch64`, `herdr-windows-x86_64.zip`, `herdr-windows-aarch64.zip`), `preview.json` (the update manifest, with the last 100 builds for SSH remote installs), `install-manifest.json`, `install.sh`, `install.ps1`, `install.cmd`, and `SHA256SUMS`. Rerunning a release is safe: an already published tag is skipped, and an interrupted publish is accepted when the published binaries match.

## Installing and updating fork builds

```bash
curl -fsSL https://github.com/puxu-msft/herdr/releases/latest/download/install.sh | sh
```

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/puxu-msft/herdr/releases/latest/download/install.ps1 | iex"
```

Fork builds are rolling preview builds (`herdr --version` prints `<version>-preview.<date>-<sha12>`) fixed to this fork's preview channel. `herdr update`, background update checks, and SSH remote installs read this fork's manifest regardless of any `[update] channel` setting, and `herdr channel set stable` is rejected. To return to upstream Herdr, run the upstream installer.

Windows ARM64 builds are native. The vendored `portable-pty` loads the bundled app-local ConPTY only on x86_64, so ARM64 uses the system ConPTY.

The fork behavior comes from build-time variables that `build.rs` validates; builds without them keep upstream behavior:

| Variable | Fork value | Effect |
|---|---|---|
| `HERDR_PREVIEW_MANIFEST_URL` | `https://github.com/<repo>/releases/latest/download/preview.json` | Preview manifest for updates and remote installs |
| `HERDR_STABLE_MANIFEST_URL` | unset | Stable manifest override |
| `HERDR_FIXED_UPDATE_CHANNEL` | `preview` | Ignore the configured update channel |

## Syncing upstream

Run `scripts/fork/sync-upstream.sh` on a clean tree. It fetches `herdrdev/herdr` `master` (adding an `upstream` remote when missing), merges it, and keeps the fork's CI:

- removes every workflow that is not `fork-*` and `.github/dependabot.yml`, including ones upstream adds;
- restores `.github/workflows/fork-*`, `.github/actions/fork-*`, `scripts/fork/`, `FORK.md`, and `scripts/release-workflows.test.ts` from the fork;
- saves upstream's changes to the replaced CI paths as a patch under `.git/fork-sync/` and lists them, so fixes worth porting are not lost silently;
- reports action pins and Zig or Bun versions that differ from upstream `ci.yml`;
- commits `merge: sync upstream master`, or stops with the remaining conflicts listed.

Review the reported patch and drift, run `just check`, then push.

Fork changes to shared upstream files are the usual merge conflicts to expect: `build.rs`, `src/build_info.rs`, `src/update.rs`, `src/remote/attach.rs`, `src/config/model.rs`, `src/cli.rs`, `.cargo/config.toml`, `distribution/install.ps1`, and `scripts/package_windows_conpty.ps1`.
