import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { clear, developmentIdentifier, developmentProfile, launchIdentifier, prepare } from "../dev-data.mjs";

const release = "dev.argus.taskmanager";

function environment() {
  const base = mkdtempSync(join(tmpdir(), "argus-dev-data-"));
  return { base, APPDATA: join(base, "roaming"), LOCALAPPDATA: join(base, "local") };
}

function files(dir, names) {
  mkdirSync(dir, { recursive: true });
  for (const name of names) writeFileSync(join(dir, name), "x");
}

test("development identifier matches the Rust derivation", () => {
  // The same vector as argus_app::config's development_profiles test.
  assert.equal(developmentIdentifier("D:/Projects/Argus", release), `${release}.devbuild.2d4d646ed1c5`);
  assert.equal(developmentIdentifier("d:\\projects\\argus", release), developmentIdentifier("D:/Projects/Argus", release));
  assert.equal(launchIdentifier(["--hidden", "--identifier", `${release}.test.a`]), `${release}.test.a`);
});

test("only development profiles not sharing the installed history folder are cleared", () => {
  const env = environment();
  try {
    for (const id of [release, "other.app.devbuild.1", `${release}.a/../b`, `${release}.`])
      assert.throws(() => developmentProfile(id, env, release));
    const id = `${release}.devbuild.abc`;
    mkdirSync(join(env.APPDATA, id), { recursive: true });
    writeFileSync(join(env.APPDATA, id, "settings.json"), JSON.stringify({ historyDir: join(env.LOCALAPPDATA, release, "history") }));
    assert.throws(() => developmentProfile(id, env, release), /shared/);
    const custom = join(env.base, "Records");
    mkdirSync(join(env.APPDATA, release), { recursive: true });
    writeFileSync(join(env.APPDATA, release, "settings.json"), JSON.stringify({ historyDir: custom }));
    writeFileSync(join(env.APPDATA, id, "settings.json"), JSON.stringify({ historyDir: custom.toUpperCase() }));
    assert.throws(() => developmentProfile(id, env, release), /shared/);
    writeFileSync(join(env.APPDATA, id, "settings.json"), JSON.stringify({ historyDir: "relative" }));
    assert.equal(developmentProfile(id, env, release).history, join(env.LOCALAPPDATA, id, "history"));
  } finally {
    rmSync(env.base, { recursive: true, force: true });
  }
});

test("a raised revision clears that kind's files once", () => {
  const env = environment();
  try {
    const id = `${release}.devbuild.abc`;
    const profile = developmentProfile(id, env, release);
    const days = ["2026-11-20.hist", "2026-11-20.proc", "2026-11-20.roll", "2026-11-20.idx", "2026-11-20.measure", "2026-11-12.merge-tmp"];
    const watch = ["process-watch.jsonl", "program-watch.v1.bin"];
    const kept = ["notes.txt", "2026-11-20.hist.bak", ".argus-history-maintenance.lock"];
    files(profile.history, [...days, ...watch, ...kept]);
    mkdirSync(join(profile.history, "2026-11-21.hist"));

    // A profile without a record is cleared once.
    assert.equal(prepare(profile, { history: 1, watch: 1 }).removed.length, days.length + watch.length);
    assert.deepEqual(readdirSync(profile.history).sort(), [...kept, "2026-11-21.hist"].sort());

    files(profile.history, [...days, ...watch]);
    assert.deepEqual(prepare(profile, { history: 1, watch: 1 }), { stale: [], removed: [] });
    assert.deepEqual(prepare(profile, { history: 1, watch: 2 }).stale, ["watch"]);
    assert.ok(days.every((name) => existsSync(join(profile.history, name))));
    assert.ok(!watch.some((name) => existsSync(join(profile.history, name))));

    assert.throws(() => clear(profile, ["settings"]));
    assert.deepEqual(clear(developmentProfile(`${release}.devbuild.empty`, env, release), ["history"]), []);
  } finally {
    rmSync(env.base, { recursive: true, force: true });
  }
});
