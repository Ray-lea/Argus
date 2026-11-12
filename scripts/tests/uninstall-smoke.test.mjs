import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { uninstallSmoke } from "../uninstall-smoke.mjs";

const windows = process.platform === "win32";
let root;
let fixture;
before(() => {
  if (!windows) return;
  root = mkdtempSync(join(tmpdir(), "argus-uninstall-test-"));
  const source = join(root, "fixture.rs");
  fixture = join(root, "fixture.exe");
  // This harmless process touches only marker files in its test directory.
  // Read the raw command line to exercise NSIS's unquoted final _?= argument.
  writeFileSync(source, String.raw`
use std::{fs, path::PathBuf, thread, time::Duration};
#[link(name = "kernel32")]
unsafe extern "system" { fn GetCommandLineW() -> *const u16; }
fn main() {
    let command = unsafe {
        let ptr = GetCommandLineW();
        let mut len = 0;
        while *ptr.add(len) != 0 { len += 1; }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    };
    let directory = PathBuf::from(command.split_once("_?=").expect("NSIS install argument").1);
    let executable = std::env::current_exe().unwrap();
    assert_ne!(executable.parent(), Some(directory.as_path()));
    fs::write(directory.join("executed-copy.txt"), executable.to_str().unwrap()).unwrap();
    let mode = fs::read_to_string(directory.join("mode.txt")).unwrap();
    if mode == "failure" { std::process::exit(23); }
    if mode == "timeout" { thread::sleep(Duration::from_secs(30)); }
    fs::remove_file(directory.join("entry.txt")).unwrap();
    // The registry marker can disappear before file cleanup is complete.
    thread::sleep(Duration::from_millis(400));
    if mode != "residue" { fs::remove_file(directory.join("argus.exe")).unwrap(); }
    fs::remove_file(directory.join("uninstall.exe")).unwrap();
}
`);
  const result = spawnSync("rustc", ["--edition=2024", source, "-o", fixture], {
    encoding: "utf8", windowsHide: true, timeout: 60_000,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
});
after(() => {
  if (root) rmSync(root, { recursive: true, force: true });
});

function installation(mode) {
  const directory = join(root, `installed ${mode} 测试`);
  mkdirSync(directory);
  copyFileSync(fixture, join(directory, "uninstall.exe"));
  writeFileSync(join(directory, "argus.exe"), "marker, not an executable");
  writeFileSync(join(directory, "mode.txt"), mode);
  const uninstallKey = join(directory, "entry.txt");
  writeFileSync(uninstallKey, "registry entry marker");
  return { directory, options: { runnerTemp: root, uninstallKey, timeoutMs: 1500 } };
}

test("uninstall waits for the external copy and preserves paths containing spaces and Unicode", { skip: !windows }, () => {
  const { directory, options } = installation("success");
  assert.match(uninstallSmoke(directory, options), /Uninstaller exit code: 0/);
  assert.equal(existsSync(options.uninstallKey), false);
  assert.equal(existsSync(join(directory, "argus.exe")), false);
  assert.equal(existsSync(join(directory, "uninstall.exe")), false);
  const executed = readFileSync(join(directory, "executed-copy.txt"), "utf8");
  assert.ok(executed.includes("argus-uninstall-smoke-"));
  assert.equal(existsSync(executed), false);
});

test("a failed uninstall reports the real process exit code", { skip: !windows }, () => {
  const { directory, options } = installation("failure");
  assert.throws(() => uninstallSmoke(directory, options), /Uninstaller failed with exit code 23/);
  assert.ok(existsSync(options.uninstallKey));
  assert.ok(existsSync(join(directory, "argus.exe")));
});

test("a removed entry does not hide a remaining application executable", { skip: !windows }, () => {
  const { directory, options } = installation("residue");
  assert.throws(() => uninstallSmoke(directory, options), (error) => {
    assert.match(error.message, /Uninstall incomplete/);
    assert.match(error.message, /registry entry remains=False/);
    assert.ok(error.message.includes(join(directory, "argus.exe")), error.message);
    return true;
  });
});

test("a stalled uninstall is terminated and its temporary copy is cleaned up", { skip: !windows }, () => {
  const { directory, options } = installation("timeout");
  assert.throws(() => uninstallSmoke(directory, options), /Uninstaller timed out/);
  assert.ok(!readdirSync(root).some((name) => name.startsWith("argus-uninstall-smoke-")));
});

test("a missing entry or an installation outside the runner temp directory fails before execution", { skip: !windows }, () => {
  const { directory, options } = installation("missing-entry");
  rmSync(options.uninstallKey);
  assert.throws(() => uninstallSmoke(directory, options), /Uninstall entry missing before uninstall/);
  assert.equal(existsSync(join(directory, "executed-copy.txt")), false);
  assert.throws(() => uninstallSmoke(root, options), /inside RUNNER_TEMP/);
  assert.throws(() => uninstallSmoke(resolve(root, "..", "outside"), options), /inside RUNNER_TEMP/);
});
