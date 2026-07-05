# TODO Grammar

## Overview

Input is organized line by line; each line has the following elements:

- Body text (TEXT)
- End-of-line metadata (TAG)
- Newline (NEWLINE)
- Indentation state (INDENT / DEDENT)
- Blank lines are ignored

## Rules

### INDENT / DEDENT

- Generated based on the indentation-level stack
- References only the leading whitespace of a line
- Blank lines do not affect INDENT/DEDENT calculation

### NEWLINE

- Generated at the end of a line
- May also be generated at EOF
- Does not appear in the parsed structure

### TAG

- Only the consecutive trailing form `@(?<name>[^\s(]+)([(](?<arg>[^)]*)[)])?` is recognized
- `arg` must not contain `)`. `@name` (argument omitted) and `@name()` (empty argument) are allowed
- TAGs must be whitespace-delimited
- Extracted greedily from the right

### TEXT

- The string remaining after removing, from the end of the line toward the front, consecutive TAGs, trailing whitespace, and a trailing colon
- A line whose TEXT is followed by a colon becomes a HEADER_LINE; otherwise it becomes a TASK_LINE
- TEXT may be an empty string (e.g. a line consisting of only TAGs)

## Structure

```
SOURCE = TASK_BLOCK
HEADING_BLOCK = HEADER_LINE [INDENT {TASK_BLOCK} DEDENT]
HEADER_LINE = TEXT ":" {TAG}
TASK_BLOCK = {HEADING_BLOCK | TASK_LINE}
TASK_LINE = TEXT {TAG}
```
