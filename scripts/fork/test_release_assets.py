import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts.fork import release_assets
from scripts.preview import ASSET_TARGETS, EXPECTED_ASSET_NAMES


class ForkInstallerTemplateTest(unittest.TestCase):
    def test_upstream_installers_still_accept_fork_defaults(self):
        installers = release_assets.fork_installers("example/fork")
        latest = "https://github.com/example/fork/releases/latest/download"

        self.assertIn(
            f'MANIFEST_URL="${{HERDR_MANIFEST_URL:-{latest}/install-manifest.json}}"',
            installers["install.sh"],
        )
        self.assertIn(f'else {{ "{latest}/preview.json" }}),', installers["install.ps1"])
        self.assertIn(f'set "INSTALLER_URL={latest}/install.ps1"\r\n', installers["install.cmd"])
        self.assertNotIn("\n", installers["install.cmd"].replace("\r\n", ""))

    def test_template_rejects_missing_anchor(self):
        with tempfile.TemporaryDirectory() as tmp:
            distribution = Path(tmp)
            for name in ("install.sh", "install.ps1", "install.cmd"):
                shutil.copyfile(release_assets.DISTRIBUTION_DIR / name, distribution / name)
            (distribution / "install.sh").write_text("#!/bin/sh\n", encoding="utf-8")
            with self.assertRaises(SystemExit):
                release_assets.fork_installers("example/fork", distribution)


class ForkReleaseAssetsTest(unittest.TestCase):
    def test_collect_rejects_digest_mismatch(self):
        with tempfile.TemporaryDirectory() as tmp:
            artifacts = Path(tmp) / "artifacts"
            for target in ASSET_TARGETS:
                name = EXPECTED_ASSET_NAMES[target]
                (artifacts / name).mkdir(parents=True)
                (artifacts / name / name).write_bytes(target.encode())
                digest = hashlib.sha256(target.encode()).hexdigest()
                (artifacts / name / f"{name}.sha256").write_text(f"{digest}  {name}\n", encoding="utf-8")

            digests = release_assets.collect(artifacts, Path(tmp) / "release")
            self.assertEqual(set(digests), set(ASSET_TARGETS))

            name = EXPECTED_ASSET_NAMES[ASSET_TARGETS[0]]
            (artifacts / name / name).write_bytes(b"tampered")
            with self.assertRaises(SystemExit):
                release_assets.collect(artifacts, Path(tmp) / "release-again")

    @unittest.skipIf(os.name == "nt", "install.sh is a Unix installer")
    def test_install_sh_installs_from_installer_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            assets = {}
            for target in ASSET_TARGETS:
                binary = root / EXPECTED_ASSET_NAMES[target]
                binary.write_bytes(f"herdr for {target}\n".encode())
                assets[target] = {
                    "url": binary.as_uri(),
                    "sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                }
            manifest = {
                "base_version": "1.2.3",
                "build_id": "2026-01-02-0123456789ab",
                "commit": "0123456789ab" * 3,
                "assets": assets,
            }
            installer_manifest = root / "install-manifest.json"
            installer_manifest.write_text(
                json.dumps(release_assets.installer_manifest(manifest), indent=2) + "\n",
                encoding="utf-8",
            )
            installer = root / "install.sh"
            installer.write_text(release_assets.fork_installers("example/fork")["install.sh"], encoding="utf-8")
            install_dir = root / "bin"

            result = subprocess.run(
                ["sh", str(installer)],
                env={
                    **os.environ,
                    "HOME": str(root),
                    "HERDR_INSTALL_DIR": str(install_dir),
                    "HERDR_MANIFEST_URL": installer_manifest.as_uri(),
                },
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("downloading v1.2.3-preview.2026-01-02-0123456789ab", result.stdout)
            installed = (install_dir / "herdr").read_bytes()
            self.assertTrue(installed.startswith(b"herdr for "), installed)


if __name__ == "__main__":
    unittest.main()
