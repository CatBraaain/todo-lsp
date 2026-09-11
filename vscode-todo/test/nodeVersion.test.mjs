import assert from "node:assert/strict";
import test from "node:test";
import { assertNode20OrLater } from "../src/nodeVersion.mjs";

test("拡張: Node.js 20以上のextension hostだけを許可する", () => {
  assert.doesNotThrow(() => assertNode20OrLater("20.0.0"));
  assert.doesNotThrow(() => assertNode20OrLater("24.1.0"));
});

test("拡張: Node.js 20未満のextension hostは一意の更新案内で拒否する", () => {
  assert.throws(() => assertNode20OrLater("18.19.0"), {
    message:
      "Todo requires Node.js 20 or later. VS Code is running Node.js 18.19.0. Update VS Code to a version that includes Node.js 20 or later.",
  });
});
