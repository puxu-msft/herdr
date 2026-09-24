#!/usr/bin/env python3
"""Assemble fork release assets from CI build artifacts.

`collect` verifies and copies the platform binaries; `installers` derives the
installer manifest and fork-pointing copies of the upstream installers from the
fork preview manifest produced by `scripts/preview.py manifest`.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.preview import ASSET_TARGETS, EXPECTED_ASSET_NAMES  # noqa: E402

DISTRIBUTION_DIR = REPO_ROOT / "distribution"


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def collect(artifacts: Path, output: Path) -> dict[str, str]:
    """Copy each expected asset out of its artifact directory after checking its digest."""
    output.mkdir(parents=True, exist_ok=True)
    digests: dict[str, str] = {}
    for target in ASSET_TARGETS:
        name = EXPECTED_ASSET_NAMES[target]
        source = artifacts / name / name
        checksum = artifacts / name / f"{name}.sha256"
        if not source.is_file() or not checksum.is_file():
            raise SystemExit(f"missing build artifact for {target}: expected {source} and {checksum}")
        recorded = checksum.read_text(encoding="utf-8-sig").split()[0].lower()
        actual = sha256_file(source)
        if recorded != actual:
            raise SystemExit(f"{name} digest {actual} does not match recorded {recorded}")
        shutil.copyfile(source, output / name)
        digests[target] = actual
    return digests


def replace_once(text: str, old: str, new: str, source: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(
            f"expected exactly one {old!r} in {source}, found {count}; update scripts/fork/release_assets.py"
        )
    return text.replace(old, new)


def installer_manifest(manifest: dict) -> dict:
    """Return the stable-shaped manifest that distribution/install.sh can parse."""
    assets = manifest["assets"]
    return {
        "version": f"{manifest['base_version']}-preview.{manifest['build_id']}",
        "build_id": manifest["build_id"],
        "commit": manifest["commit"],
        "assets": {target: assets[target]["url"] for target in ASSET_TARGETS},
        "sha256": {target: assets[target]["sha256"] for target in ASSET_TARGETS},
    }


def fork_installers(repo: str, distribution: Path = DISTRIBUTION_DIR) -> dict[str, str]:
    """Return copies of the upstream installers that default to this fork's releases."""
    latest = f"https://github.com/{repo}/releases/latest/download"
    install_sh = replace_once(
        (distribution / "install.sh").read_text(encoding="utf-8"),
        'MANIFEST_URL="https://herdr.dev/latest.json"',
        f'MANIFEST_URL="${{HERDR_MANIFEST_URL:-{latest}/install-manifest.json}}"',
        "distribution/install.sh",
    )
    # A preview.json default also makes install.ps1 select the preview channel.
    install_ps1 = replace_once(
        (distribution / "install.ps1").read_text(encoding="utf-8"),
        "[string]$ManifestUrl = $env:HERDR_MANIFEST_URL,",
        "[string]$ManifestUrl = $(if ($env:HERDR_MANIFEST_URL) { $env:HERDR_MANIFEST_URL } "
        f'else {{ "{latest}/preview.json" }}),',
        "distribution/install.ps1",
    )
    install_cmd = replace_once(
        (distribution / "install.cmd").read_text(encoding="utf-8"),
        'set "INSTALLER_URL=https://herdr.dev/install.ps1"',
        f'set "INSTALLER_URL={latest}/install.ps1"',
        "distribution/install.cmd",
    )
    install_cmd = install_cmd.replace("\r\n", "\n").replace("\n", "\r\n")
    return {"install.sh": install_sh, "install.ps1": install_ps1, "install.cmd": install_cmd}


def write_checksums(output: Path) -> None:
    lines = [
        f"{sha256_file(path)}  {path.name}"
        for path in sorted(output.iterdir(), key=lambda item: item.name)
        if path.is_file() and path.name != "SHA256SUMS"
    ]
    (output / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


def cmd_collect(args: argparse.Namespace) -> int:
    digests = collect(Path(args.artifacts), Path(args.output))
    Path(args.sha_json).write_text(json.dumps(digests, indent=2) + "\n", encoding="utf-8")
    return 0


def cmd_installers(args: argparse.Namespace) -> int:
    output = Path(args.output)
    manifest = json.loads(Path(args.manifest).read_text(encoding="utf-8"))
    (output / "install-manifest.json").write_text(
        json.dumps(installer_manifest(manifest), indent=2) + "\n", encoding="utf-8"
    )
    for name, content in fork_installers(args.repo).items():
        (output / name).write_bytes(content.encode("utf-8"))
    write_checksums(output)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(required=True)

    collect_parser = sub.add_parser("collect", help="verify and copy platform binaries")
    collect_parser.add_argument("--artifacts", required=True)
    collect_parser.add_argument("--output", required=True)
    collect_parser.add_argument("--sha-json", required=True)
    collect_parser.set_defaults(func=cmd_collect)

    installers_parser = sub.add_parser("installers", help="write installer assets and SHA256SUMS")
    installers_parser.add_argument("--repo", required=True)
    installers_parser.add_argument("--manifest", required=True)
    installers_parser.add_argument("--output", required=True)
    installers_parser.set_defaults(func=cmd_installers)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
