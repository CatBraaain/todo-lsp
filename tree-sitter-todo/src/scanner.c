#include "tree_sitter/array.h"
#include "tree_sitter/parser.h"

#include <stdint.h>

// NEWLINE / INDENT / DEDENT のロジックは tree-sitter 公式 Python grammar
// (src/scanner.c) から移植。文字列・括弧・コメント・行継続の処理は本プロジェクト
// では不要なため削除し、TEXT 処理だけを独自に追加している。

enum TokenType
{
  NEWLINE,
  INDENT,
  DEDENT,
  TEXT,
};

typedef struct
{
  Array(uint16_t) indents;
} Scanner;

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

// --- TEXT 境界判定 -------------------------------------------------------
// TAG の境界判定。name は空白・タブ・"("・改行・EOF で終わる。
static bool is_name_boundary(TSLexer *lexer)
{
  return lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
         lexer->lookahead == '(' || lexer->lookahead == '\n' ||
         lexer->eof(lexer);
}

// 1 つの TAG を消費する。先頭の "@" から ")" まで。成立で true。
static bool scan_one_tag(TSLexer *lexer)
{
  lexer->advance(lexer, false);

  if (is_name_boundary(lexer))
  {
    return false;
  }

  while (!is_name_boundary(lexer))
  {
    lexer->advance(lexer, false);
  }

  if (lexer->lookahead == '(')
  {
    lexer->advance(lexer, false);
    while (lexer->lookahead != ')' && lexer->lookahead != '\n' &&
           !lexer->eof(lexer))
    {
      lexer->advance(lexer, false);
    }
    if (lexer->lookahead != ')')
    {
      return false;
    }
    lexer->advance(lexer, false);
  }

  return true;
}

// 行末に向かって "空白? @name(arg?) (空白)*" の連続 + 行末/EOF を検証。
static bool scan_trailing_tags(TSLexer *lexer)
{
  while (true)
  {
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t')
    {
      lexer->advance(lexer, false);
    }

    if (lexer->lookahead != '@')
    {
      return false;
    }

    if (!scan_one_tag(lexer))
    {
      return false;
    }

    if (lexer->lookahead == '\n' || lexer->eof(lexer))
    {
      return true;
    }

    if (lexer->lookahead != ' ' && lexer->lookahead != '\t')
    {
      return false;
    }
  }
  return false;
}

// ":" の後に "TAG*(0 個可) + 行末/EOF" が続くか検証。
static bool scan_trailing_colon(TSLexer *lexer)
{
  lexer->advance(lexer, false);

  while (true)
  {
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t')
    {
      lexer->advance(lexer, false);
    }

    if (lexer->lookahead == '\n' || lexer->eof(lexer))
    {
      return true;
    }

    if (lexer->lookahead != '@' || !scan_one_tag(lexer))
    {
      return false;
    }
  }
  return false;
}

// 本文を走査し、末尾の TAG 連続・trailing colon を取り除いた位置で TEXT 終端を確定。
// TEXT が空（タグのみの行など）のときは false。
static bool scan_text(TSLexer *lexer)
{
  bool text_end_set = false;
  bool prev_was_space = true;

  while (true)
  {
    if (lexer->lookahead == '\n' || lexer->eof(lexer))
    {
      break;
    }

    if (lexer->lookahead == ' ' || lexer->lookahead == '\t')
    {
      lexer->mark_end(lexer);
      text_end_set = true;
      lexer->advance(lexer, false);
      prev_was_space = true;
      continue;
    }

    if (lexer->lookahead == '@' && prev_was_space)
    {
      if (!text_end_set)
      {
        return false;
      }
      if (scan_trailing_tags(lexer))
      {
        return text_end_set;
      }
      lexer->mark_end(lexer);
      text_end_set = true;
      prev_was_space = false;
      continue;
    }

    if (lexer->lookahead == ':')
    {
      if (!text_end_set)
      {
        return false;
      }
      if (scan_trailing_colon(lexer))
      {
        return text_end_set;
      }
      lexer->mark_end(lexer);
      text_end_set = true;
      prev_was_space = false;
      continue;
    }

    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    text_end_set = true;
    prev_was_space = false;
  }

  if (!text_end_set)
  {
    lexer->mark_end(lexer);
  }
  return text_end_set;
}

static bool is_body_start(TSLexer *lexer)
{
  return lexer->lookahead != '\n' && !lexer->eof(lexer) &&
         lexer->lookahead != ' ' && lexer->lookahead != '\t' &&
         lexer->lookahead != '\r' && lexer->lookahead != '\f';
}

// --- scan 本体 -----------------------------------------------------------
bool tree_sitter_todo_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid_symbols)
{
  Scanner *scanner = (Scanner *)payload;

  bool error_recovery_mode = valid_symbols[NEWLINE] && valid_symbols[INDENT];

  // 改行文字は NEWLINE。\n を含む非ゼロ幅にして repeat($._newline) の
  // 無限ループを防ぐ。後続の行頭空白は次の scan で INDENT/DEDENT として処理。
  if (!error_recovery_mode && valid_symbols[NEWLINE] && lexer->lookahead == '\n')
  {
    while (lexer->lookahead == '\n')
    {
      advance(lexer);
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = NEWLINE;
    return true;
  }

  // 行頭空白で INDENT / DEDENT。scan 開始位置を mark_end で固定してゼロ幅にし、
  // 次の scan が同じ行頭から indent_length を再計算できるようにする。
  // インデント単位は SPEC.md のインデントレベル定義に合わせる:
  // 1レベル = スペース4個。タブは直前のスペース3個までと合わせて
  // 1レベル（次の4の倍数へ進む）として数える。
  lexer->mark_end(lexer);
  uint16_t indent_length = 0;
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t')
  {
    if (lexer->lookahead == '\t')
    {
      indent_length = (uint16_t)(((indent_length / 4) + 1) * 4);
    }
    else
    {
      indent_length += 1;
    }
    skip(lexer);
  }
  if (scanner->indents.size > 0)
  {
    uint16_t current_indent_length = *array_back(&scanner->indents);

    if (valid_symbols[INDENT] && indent_length > current_indent_length)
    {
      array_push(&scanner->indents, indent_length);
      lexer->result_symbol = INDENT;
      return true;
    }

    if (valid_symbols[DEDENT] && indent_length < current_indent_length)
    {
      (void)array_pop(&scanner->indents);
      lexer->result_symbol = DEDENT;
      return true;
    }
  }

  // 本文。
  if (!error_recovery_mode && valid_symbols[TEXT] && is_body_start(lexer))
  {
    if (scan_text(lexer))
    {
      lexer->result_symbol = TEXT;
      return true;
    }
  }

  // EOF でも NEWLINE。
  if (valid_symbols[NEWLINE] && lexer->eof(lexer))
  {
    lexer->result_symbol = NEWLINE;
    return true;
  }

  return false;
}

// --- serialize / deserialize / create / destroy（Python 版と同一ロジック） ---
unsigned tree_sitter_todo_external_scanner_serialize(void *payload, char *buffer)
{
  Scanner *scanner = (Scanner *)payload;

  size_t size = 0;

  uint32_t iter = 1;
  for (; iter < scanner->indents.size && size + 1 < TREE_SITTER_SERIALIZATION_BUFFER_SIZE; ++iter)
  {
    uint16_t indent_value = *array_get(&scanner->indents, iter);
    buffer[size++] = (char)(indent_value & 0xFF);
    buffer[size++] = (char)((indent_value >> 8) & 0xFF);
  }

  return size;
}

void tree_sitter_todo_external_scanner_deserialize(void *payload, const char *buffer, unsigned length)
{
  Scanner *scanner = (Scanner *)payload;

  array_delete(&scanner->indents);
  array_push(&scanner->indents, 0);

  if (length > 0)
  {
    size_t size = 0;

    for (; size + 1 < length; size += 2)
    {
      uint16_t indent_value = (unsigned char)buffer[size] | ((unsigned char)buffer[size + 1] << 8);
      array_push(&scanner->indents, indent_value);
    }
  }
}

void *tree_sitter_todo_external_scanner_create()
{
  Scanner *scanner = calloc(1, sizeof(Scanner));
  array_init(&scanner->indents);
  tree_sitter_todo_external_scanner_deserialize(scanner, NULL, 0);
  return scanner;
}

void tree_sitter_todo_external_scanner_destroy(void *payload)
{
  Scanner *scanner = (Scanner *)payload;
  array_delete(&scanner->indents);
  free(scanner);
}
