/// <reference types="./grammar.d.ts" />
// @ts-check

export default grammar({
  name: "todo",
  extras: ($) => [$.comment, /[ \t]/],
  externals: ($) => [$._newline, $.indent, $.dedent, $.text],
  rules: {
    source_file: ($) => seq(optional($._newline), repeat(choice($.heading_block, $.task_line))),

    heading_block: ($) =>
      seq($.heading_line, optional(seq($.indent, $.task_block, $.dedent))),

    heading_line: ($) =>
      prec(1, prec.left(seq(field("text", optional($.text)), $.colon, repeat($.tag), $._newline))),

    colon: () => ":",

    task_block: ($) => repeat1(choice($.heading_block, $.task_line)),

    task_line: ($) =>
      prec(1, prec.left(seq(
        choice(
          seq(field("text", $.text), repeat($.tag)),
          repeat1($.tag)
        ),
        $._newline
      ))),

    tag: ($) => seq("@", field("name", $.name), optional(field("arg", $.arg))),
    name: () => token(/[^\s(]+/),
    arg: () => token(seq("(", /[^)]*/, ")")),

    comment: () => token(seq("#", /[^\n]*/)),
  },
});
