import assert from "node:assert/strict";
import { test } from "node:test";
import { HistoryFollow } from "../src/lib/history-follow.ts";
import { enabledToolbarButtons } from "../src/lib/toolbar-modules.ts";

test("history follows by default and resumes only after 60 seconds of inactivity", () => {
  const follow = new HistoryFollow();
  assert.equal(follow.paused, false);
  follow.pause(1000);
  assert.equal(follow.resumeIfIdle(60_999, false), false);
  assert.equal(follow.paused, true);
  assert.equal(follow.resumeIfIdle(61_000, false), true);
  assert.equal(follow.paused, false);
});

test("further navigation resets the idle deadline", () => {
  const follow = new HistoryFollow();
  follow.pause(0);
  follow.pause(50_000);
  assert.equal(follow.resumeIfIdle(60_000, false), false);
  assert.equal(follow.resumeIfIdle(110_000, false), true);
});

test("holding a selection or editing a page number prevents timed resume", () => {
  const follow = new HistoryFollow();
  follow.pause(0);
  assert.equal(follow.resumeIfIdle(70_000, true), false);
  assert.equal(follow.resumeIfIdle(129_999, false), false);
  assert.equal(follow.resumeIfIdle(130_000, false), true);
});

test("return to latest immediately clears pause and cancels pending automatic resume", () => {
  const follow = new HistoryFollow();
  follow.pause(0);
  follow.resume();
  assert.equal(follow.paused, false);
  assert.equal(follow.resumeIfIdle(60_000, false), false);
});

test("actual toolbar preserves module order, groups selection buttons, and omits disabled features", () => {
  const settings = { toolbar_order: ["captions", "selection", "ocr"], captions: { enabled: true }, selection: { enabled: true }, ocr: { enabled: false } };
  assert.deepEqual(enabledToolbarButtons(settings), ["captions", "translate", "copy"]);
  settings.selection.enabled = false;
  assert.deepEqual(enabledToolbarButtons(settings), ["captions"]);
});
