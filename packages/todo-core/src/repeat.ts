import { cronFor } from "./dates.js";
import { formatDocument, joinLines, splitLines } from "./format.js";
import { indentForLevel, isBlank, parseLine, tagArg, taskTextOf } from "./line.js";

interface Node {
  line: number;
  units: number;
  level: number;
  parent: number | undefined;
}

/** Generate the most recent task for every valid `@repeat` definition. */
export function repeatTasks(source: string, now = new Date(), tabSize = 4): string {
  const lines = splitLines(source);
  if (lines.length === 0) return source;

  const definitions: Array<[string, Date]> = [];
  for (const line of lines) {
    const expression = tagArg(parseLine(line), "repeat");
    const previous = expression === undefined ? undefined : previousOccurrence(expression, now);
    if (previous !== undefined) definitions.push([line, previous]);
  }

  for (const [definition, previous] of definitions) {
    processDefinition(lines, definition, previous, tabSize);
  }

  return formatDocument(joinLines(lines, source), tabSize);
}

function processDefinition(
  lines: string[],
  definition: string,
  previous: Date,
  tabSize: number,
): void {
  const parts = parseLine(definition);

  const start = tagArg(parts, "start");
  const startDate = start === undefined ? undefined : parseLocalDate(start);
  if (startDate !== undefined && startDate.getTime() > previous.getTime()) return;

  const path = taskTextOf(parts, definition)
    .split("/")
    .map((part) => part.trim())
    .filter(Boolean);
  const name = path.pop();
  if (name === undefined) return;

  let container: number | undefined;
  for (const parentName of path) {
    const child = findChild(lines, container, parentName, tabSize);
    if (child !== undefined) {
      container = child;
      continue;
    }

    const insertAt = blockEnd(lines, container, tabSize) + 1;
    lines.splice(insertAt, 0, `${indentForContainer(lines, container, tabSize)}${parentName}`);
    container = nodes(lines, tabSize).findIndex((node) => node.line === insertAt);
  }

  const exists = lines.some((line) => {
    const candidate = parseLine(line);
    const candidateStart = tagArg(candidate, "start");
    return (
      taskTextOf(candidate, line) === name &&
      candidateStart !== undefined &&
      parseLocalDate(candidateStart)?.getTime() === previous.getTime()
    );
  });
  if (exists) return;

  const insertAt = blockEnd(lines, container, tabSize) + 1;
  lines.splice(
    insertAt,
    0,
    `${indentForContainer(lines, container, tabSize)}${name} @start(${renderLocalDate(previous)})`,
  );
}

function previousOccurrence(expression: string, now: Date): Date | undefined {
  const previous = cronFor(expression)?.previousRuns(1, now)[0];
  if (previous === undefined) return undefined;
  return new Date(
    previous.getFullYear(),
    previous.getMonth(),
    previous.getDate(),
    previous.getHours(),
    previous.getMinutes(),
  );
}

function nodes(lines: readonly string[], tabSize: number): Node[] {
  const nodes: Node[] = [];
  for (const [line, text] of lines.entries()) {
    const parts = parseLine(text, tabSize);
    if (isBlank(parts)) continue;
    let parent: number | undefined;
    for (let index = nodes.length - 1; index >= 0; index--) {
      if (nodes[index].units < parts.units) {
        parent = index;
        break;
      }
    }
    nodes.push({ line, units: parts.units, level: parts.level, parent });
  }
  return nodes;
}

function findChild(
  lines: readonly string[],
  container: number | undefined,
  name: string,
  tabSize: number,
): number | undefined {
  const structure = nodes(lines, tabSize);
  const index = structure.findIndex((node) => {
    if (node.parent !== container) return false;
    const line = lines[node.line];
    return taskTextOf(parseLine(line), line).includes(name);
  });
  return index === -1 ? undefined : index;
}

function blockEnd(
  lines: readonly string[],
  container: number | undefined,
  tabSize: number,
): number {
  const structure = nodes(lines, tabSize);
  if (container === undefined) return structure.at(-1)?.line ?? -1;

  const start = structure[container];
  let end = start.line;
  for (const node of structure) {
    if (node.line <= start.line) continue;
    if (node.units > start.units) end = node.line;
    else break;
  }
  return end;
}

function indentForContainer(
  lines: readonly string[],
  container: number | undefined,
  tabSize: number,
): string {
  return container === undefined
    ? ""
    : indentForLevel(nodes(lines, tabSize)[container].level + 1, tabSize);
}

function parseLocalDate(text: string): Date | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})(?: (\d{2}):(\d{2}))?$/.exec(text.trim());
  if (!match) return undefined;

  const date = new Date(
    Number(match[1]),
    Number(match[2]) - 1,
    Number(match[3]),
    Number(match[4] ?? 0),
    Number(match[5] ?? 0),
  );
  return date.getFullYear() === Number(match[1]) &&
    date.getMonth() === Number(match[2]) - 1 &&
    date.getDate() === Number(match[3]) &&
    date.getHours() === Number(match[4] ?? 0) &&
    date.getMinutes() === Number(match[5] ?? 0)
    ? date
    : undefined;
}

function renderLocalDate(date: Date): string {
  const day = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
  return date.getHours() === 0 && date.getMinutes() === 0
    ? day
    : `${day} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}
