// Document links for closed `<https://…>`, `<http://…>` and `<ftp://…>`
// spans in plain task-line bodies (SPEC「Document links」). Link ranges
// obey the same display precedence as inline styling: gray / Archive /
// heading lines do not expose individual links. Positions are UTF-16 code
// units (SPEC「LSP の位置」).

import { grayOf, isArchiveHeading, isBlank, isHeading, parseLine } from "./line.js";
import type { DocumentLink } from "./lsp-types.js";

const schemes = ["https://", "http://", "ftp://"];

/** URI pattern per RFC 3986 appendix B, mirroring Rust's `Uri::from_str`
 * acceptance: scheme, hier-part, optional query and fragment. */
const uriPattern = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\s?#]*(?:\?[^\s#]*)?(?:#[^\s]*)?$/;

/** Build document links for the closed URL spans in plain-line bodies. */
export function documentLinks(source: string): DocumentLink[] {
  const out: DocumentLink[] = [];
  const lines = source.split("\n");
  for (let lineIdx = 0; lineIdx < lines.length; lineIdx++) {
    const rawLine = lines[lineIdx];
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    const parts = parseLine(line);
    if (
      isBlank(parts) ||
      isArchiveHeading(parts, line) ||
      grayOf(parts) !== undefined ||
      isHeading(parts)
    ) {
      continue;
    }
    const [textStart, textEnd] = parts.textRange;
    collectDocumentLinks(line.slice(textStart, textEnd), textStart, lineIdx, out);
  }
  return out;
}

function collectDocumentLinks(
  text: string,
  offset: number,
  lineIdx: number,
  out: DocumentLink[],
): void {
  const bytes = Buffer.from(text, "utf8");
  const utf16OfByte = (byteIndex: number): number =>
    bytes.subarray(0, byteIndex).toString("utf8").length;

  let i = 0;
  while (i < bytes.length) {
    if (bytes[i] !== 0x3c /* < */) {
      i += 1;
      continue;
    }
    const rest = bytes.subarray(i + 1);
    const matchedScheme = schemes.find((scheme) => startsWithAscii(rest, scheme));
    if (!matchedScheme) {
      i += 1;
      continue;
    }
    const gt = rest.indexOf(0x3e /* > */);
    if (gt === -1) {
      i += 1; // unclosed URL: no link
      continue;
    }
    const end = i + gt + 2;
    const url = bytes.subarray(i + 1, end - 1).toString("utf8");
    if (uriPattern.test(url)) {
      const start16 = utf16OfByte(i);
      const end16 = utf16OfByte(end);
      out.push({
        range: {
          start: { line: lineIdx, character: offset + start16 },
          end: { line: lineIdx, character: offset + end16 },
        },
        target: url,
      });
    }
    i = end;
  }
}

function startsWithAscii(bytes: Buffer, prefix: string): boolean {
  const expected = Buffer.from(prefix, "ascii");
  return bytes.subarray(0, expected.length).equals(expected);
}
