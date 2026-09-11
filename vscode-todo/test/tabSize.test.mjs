// §タブサイズ The editor's tab-size resolution (src/tabSize.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { effectiveTabSize } from "../src/tabSize.mjs";

test("タブサイズ: uses the editor's resolved tabSize option when it is a number", () => {
  assert.equal(
    effectiveTabSize({ options: { tabSize: 8 } }, () => 2),
    8,
  );
});

test("タブサイズ: falls back to the editor.tabSize setting when the option is unset", () => {
  assert.equal(
    effectiveTabSize({ options: {} }, () => 2),
    2,
  );
});

test("タブサイズ: falls back to 4 when neither the option nor the setting yields a value", () => {
  assert.equal(
    effectiveTabSize({ options: {} }, () => undefined),
    4,
  );
});

test("タブサイズ: treats non-positive values as unavailable and falls back", () => {
  assert.equal(
    effectiveTabSize({ options: { tabSize: 0 } }, () => 2),
    2,
  );
  assert.equal(
    effectiveTabSize({ options: { tabSize: -4 } }, () => -1),
    4,
  );
});
