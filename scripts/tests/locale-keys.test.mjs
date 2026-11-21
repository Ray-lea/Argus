import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { inserted, plan, removed, splitKey, write } from "../lib/locale-keys.mjs";

test("a key names its catalog file and a path in it", () => {
  assert.deepEqual(splitKey("tasks.explorer.note"), { namespace: "tasks", path: ["explorer", "note"] });
  assert.throws(() => splitKey("tasks"), /expected <file>\.<key>/);
  assert.throws(() => splitKey("tasks..note"), /expected <file>\.<key>/);
  assert.throws(() => splitKey("levels.3"), /digits alone/);
});

test("a new key keeps the place it was given", () => {
  const catalog = { title: "T", explorer: { title: "E", confirm: "C", done: "D" }, last: "L" };
  assert.deepEqual(Object.keys(inserted(catalog, ["explorer", "note"], "N", { after: "confirm" }).explorer), ["title", "confirm", "note", "done"]);
  assert.deepEqual(Object.keys(inserted(catalog, ["explorer", "note"], "N", { before: "title" }).explorer), ["note", "title", "confirm", "done"]);
  assert.deepEqual(Object.keys(inserted(catalog, ["explorer", "note"], "N").explorer), ["title", "confirm", "done", "note"]);
  // The other keys, and the original, are untouched.
  assert.deepEqual(Object.keys(inserted(catalog, ["extra"], "X", { after: "title" })), ["title", "extra", "explorer", "last"]);
  assert.deepEqual(Object.keys(catalog.explorer), ["title", "confirm", "done"]);
  // A group that does not exist yet is created at the end.
  assert.deepEqual(inserted(catalog, ["fresh", "inner", "text"], { one: "a", other: "b" }).fresh, { inner: { text: { one: "a", other: "b" } } });

  assert.throws(() => inserted(catalog, ["title"], "again"), /already exists/);
  assert.throws(() => inserted(catalog, ["explorer", "note"], "N", { after: "missing" }), /no "missing" beside note/);
  assert.throws(() => inserted(catalog, ["title", "inner"], "N"), /title is a text, not a group/);
  assert.throws(() => inserted(catalog, ["empty"], ""), /non-empty string or plural forms/);
  assert.throws(() => inserted(catalog, ["bad"], { several: "x" }), /non-empty string or plural forms/);
  assert.throws(() => inserted(catalog, ["both"], "x", { after: "title", before: "last" }), /not both/);
});

test("a removed key takes an emptied group with it", () => {
  const catalog = { title: "T", explorer: { only: "O" }, panel: { a: "A", b: "B" } };
  assert.deepEqual(removed(catalog, ["explorer", "only"]), { title: "T", panel: { a: "A", b: "B" } });
  assert.deepEqual(removed(catalog, ["panel", "a"]), { title: "T", explorer: { only: "O" }, panel: { b: "B" } });
  assert.throws(() => removed(catalog, ["panel", "c"]), /panel\.c|c does not exist/);
  assert.throws(() => removed(catalog, ["title", "x"]), /not a group/);
});

test("every language gets the key, or none does", () => {
  const root = mkdtempSync(join(tmpdir(), "argus-locales-"));
  try {
    const original = { "zh-CN": { title: "标题", confirm: "确认" }, "en-US": { title: "Title", confirm: "Confirm" }, "ru-RU": { title: "Заголовок", confirm: "Подтвердить" } };
    for (const [locale, catalog] of Object.entries(original)) {
      mkdirSync(join(root, locale));
      writeFileSync(join(root, locale, "tasks.json"), `${JSON.stringify(catalog, null, 2)}\n`);
    }
    const read = (locale) => JSON.parse(readFileSync(join(root, locale, "tasks.json"), "utf8"));
    const texts = { "zh-CN": "已结束 {count} 个进程", "en-US": { one: "Ended {count} process", other: "Ended {count} processes" }, "ru-RU": { one: "Завершён {count} процесс", few: "Завершено {count} процесса", many: "Завершено {count} процессов", other: "Завершено {count} процесса" } };

    const changes = plan(root, [{ add: "tasks.ended", texts, after: "title" }]);
    assert.equal(Object.keys(changes.next).length, 3);
    assert.deepEqual(read("en-US"), original["en-US"], "planning writes nothing");
    write(changes.next);
    assert.deepEqual(Object.keys(read("zh-CN")), ["title", "ended", "confirm"]);
    assert.deepEqual(read("en-US").ended, texts["en-US"]);
    assert(readFileSync(join(root, "ru-RU", "tasks.json"), "utf8").endsWith("}\n"));
    write(changes.previous);
    assert.deepEqual(read("ru-RU"), original["ru-RU"]);

    assert.throws(() => plan(root, [{ add: "tasks.partial", texts: { "zh-CN": "甲", "en-US": "A" } }]), /no text for ru-RU/);
    assert.throws(() => plan(root, [{ add: "tasks.extra", texts: { ...texts, "ko-KR": "x" } }]), /ko-KR is not a language/);
    assert.throws(() => plan(root, [{ add: "tasks.count", texts: { "zh-CN": "{count} 项", "en-US": "{total} items", "ru-RU": "{count}" } }]), /en-US has placeholders \{total\}/);
    assert.throws(() => plan(root, [{ add: "tasks.plural", texts: { ...texts, "zh-CN": { other: "{count}" } } }]), /source has no plural forms/);
    assert.throws(() => plan(root, [{ add: "missing.key", texts: { "zh-CN": "甲", "en-US": "A", "ru-RU": "А" } }]), /missing\.json does not exist/);
    assert.throws(() => plan(root, [{ add: "tasks.title", texts: { "zh-CN": "甲", "en-US": "A", "ru-RU": "А" } }]), /tasks\.json: title already exists/);

    // Two edits in one go see each other; a removal is planned the same way.
    const both = plan(root, [{ add: "tasks.group.first", texts: { "zh-CN": "一", "en-US": "One", "ru-RU": "Один" } }, { add: "tasks.group.second", texts: { "zh-CN": "二", "en-US": "Two", "ru-RU": "Два" }, before: "first" }, { remove: "tasks.confirm" }]);
    write(both.next);
    assert.deepEqual(read("en-US"), { title: "Title", group: { second: "Two", first: "One" } });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
