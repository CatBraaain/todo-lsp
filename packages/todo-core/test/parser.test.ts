import assert from "node:assert/strict";
import test from "node:test";
import type * as TodoCore from "../src/index.js";

const sampleDocument = `Inbox:
  buy milk
  call mom @done(2024-01-01)
  Project:
    draft spec @priority(high)
    review @done
  wrap up
Archive:
  old task
`;

test("parses the complete specification sample without errors", async () => {
  const { createTodoParser } = (await import(
    new URL("../index.js", import.meta.url).href
  )) as typeof TodoCore;
  const tree = (await createTodoParser()).parse(sampleDocument);

  assert.equal(tree.rootNode.hasError, false);
  assert.equal(tree.rootNode.type, "source_file");
  assert.deepEqual(
    tree.rootNode.namedChildren.map((node) => node?.type),
    ["heading_block", "heading_block"],
  );
  assert.deepEqual(
    tree.rootNode.namedChildren[0]?.namedChildren.map((node) => node?.type),
    ["heading_line", "indent", "task_block", "dedent"],
  );
  assert.equal(
    tree.rootNode.namedChildren[0]?.namedChildren[0]?.childForFieldName("text")?.text,
    "Inbox",
  );
  assert.deepEqual(
    tree.rootNode.namedChildren[0]?.namedChildren[2]?.namedChildren.map((node) => node?.type),
    ["task_line", "task_line", "heading_block", "task_line"],
  );
});
