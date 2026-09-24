import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

// Fork-owned: scripts/fork/sync-upstream.sh keeps this file and the fork workflows
// when merging upstream, whose publishing workflows do not exist in this fork.
const workflowDir = new URL("../.github/workflows/", import.meta.url);
const repoRoot = fileURLToPath(new URL("..", import.meta.url));
const load = (name: string): any => Bun.YAML.parse(readFileSync(new URL(`${name}.yml`, workflowDir), "utf8"));
const ci = load("fork-ci");
const checks = load("fork-checks");
const build = load("fork-build");
const release = load("fork-release");

describe("fork workflows", () => {
  test("only fork-owned workflows exist", () => {
    const workflows = readdirSync(workflowDir).filter((name) => /\.ya?ml$/.test(name));
    expect(workflows.length).toBeGreaterThan(0);
    expect(workflows.filter((name) => !name.startsWith("fork-"))).toEqual([]);
  });

  test("pushes and external pull requests run the shared checks on every platform", () => {
    expect(ci.on.push.branches).toEqual(["**"]);
    expect(ci.on.pull_request).toBeDefined();
    expect(ci.jobs.checks.uses).toBe("./.github/workflows/fork-checks.yml");
    expect(Object.keys(checks.on)).toEqual(["workflow_call"]);
    expect(checks.jobs.check.strategy.matrix.include.map((entry: any) => entry.os)).toEqual([
      "ubuntu-latest",
      "macos-latest",
      "windows-latest",
      "windows-11-arm",
    ]);
    expect(checks.jobs["windows-package"].strategy.matrix.include.map((entry: any) => entry.architecture)).toEqual([
      "x86_64",
      "arm64",
    ]);
  });

  test("release builds exactly the assets the manifest tooling publishes", () => {
    const python = process.platform === "win32" ? "python" : "python3";
    const result = spawnSync(
      python,
      ["-c", "import json, scripts.preview as p; print(json.dumps(sorted(p.EXPECTED_ASSET_NAMES.values())))"],
      { cwd: repoRoot, encoding: "utf8" },
    );
    expect(result.status).toBe(0);
    const assets = build.jobs.build.strategy.matrix.include.map((entry: any) => entry.asset).sort();
    expect(assets).toEqual(JSON.parse(result.stdout));
  });

  test("publishing waits for checks and builds that only follow this fork", () => {
    expect(release.on.push).toEqual({ tags: ["fork-*"] });
    expect(release.on).toHaveProperty("workflow_dispatch");
    expect(release.permissions).toEqual({ contents: "read" });
    expect(release.jobs.checks.uses).toBe("./.github/workflows/fork-checks.yml");
    expect(release.jobs.build.uses).toBe("./.github/workflows/fork-build.yml");
    expect(release.jobs.publish.needs).toEqual(["plan", "checks", "build"]);
    expect(release.jobs.publish.permissions).toEqual({ contents: "write" });
    expect(release.jobs["verify-install"].needs).toEqual(["plan", "publish"]);

    const env = build.jobs.build.env;
    expect(env.HERDR_BUILD_CHANNEL).toBe("preview");
    expect(env.HERDR_FIXED_UPDATE_CHANNEL).toBe("preview");
    expect(env.HERDR_PREVIEW_MANIFEST_URL).toBe(
      "https://github.com/${{ github.repository }}/releases/latest/download/preview.json",
    );
  });

  test("release arguments are not interpolated into executable shell text", () => {
    const input = `untrusted'\"$(echo unexpected-command)`;
    for (const args of [
      ["preview", input],
      ["release-prepare", input, input],
      ["release-publish", input, input],
      ["release", input, input],
    ]) {
      const result = spawnSync("just", ["--dry-run", ...args], { encoding: "utf8" });
      expect(result.status).toBe(0);
      expect(result.stdout + result.stderr).not.toContain(input);
      expect(result.stdout + result.stderr).not.toContain("unexpected-command");
    }
  });
});
