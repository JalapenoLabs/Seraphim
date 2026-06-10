// Lightweight JSON syntax highlighter for the activity log.
//
// Agent log lines often embed JSON, e.g. `ToolSearch({"query":"..."})` or a raw
// `[{"tool_name":"WebFetch"}]` result. This scans a line for any *valid* JSON
// value, lexes it into colored tokens, and leaves the surrounding text plain.
// The lexer only assigns a semantic `kind` (and bracket nesting `depth`); the
// component maps those to theme colors, so all Tailwind classes live in markup.

export type TokenKind =
  | 'plain'
  | 'key'
  | 'string'
  | 'number'
  | 'keyword' // true / false / null
  | 'punct' // : and ,
  | 'bracket' // { } [ ] (colored by `depth` for rainbow nesting)

export type HighlightToken = {
  text: string
  kind: TokenKind
  /** Nesting depth for `bracket` tokens, so each level can get its own color. */
  depth?: number
}

/**
 * Splits `line` into tokens: stretches of plain text interleaved with any valid
 * JSON values found within it, the latter lexed into colored tokens.
 */
export function highlightJson(line: string): HighlightToken[] {
  const tokens: HighlightToken[] = []
  let cursor = 0 // start of the current run of plain text
  let i = 0

  const flushPlain = (end: number) => {
    if (end > cursor) {
      tokens.push({ text: line.slice(cursor, end), kind: 'plain' })
    }
  }

  while (i < line.length) {
    const char = line[i]
    if (char === '{' || char === '[') {
      const end = jsonValueEnd(line, i)
      if (end !== null) {
        flushPlain(i)
        lexJson(line.slice(i, end), tokens)
        i = end
        cursor = end
        continue
      }
    }
    i += 1
  }

  flushPlain(line.length)
  return tokens
}

/**
 * If a balanced, `JSON.parse`-able value starts at `start` (`{` or `[`), returns
 * the index just past it; otherwise `null`. String contents (and their escapes)
 * are skipped so braces inside strings don't throw off the balance.
 */
function jsonValueEnd(text: string, start: number): number | null {
  let depth = 0
  let inString = false
  let escaped = false

  for (let i = start; i < text.length; i += 1) {
    const char = text[i]
    if (inString) {
      if (escaped) {
        escaped = false
      } else if (char === '\\') {
        escaped = true
      } else if (char === '"') {
        inString = false
      }
      continue
    }
    if (char === '"') {
      inString = true
    } else if (char === '{' || char === '[') {
      depth += 1
    } else if (char === '}' || char === ']') {
      depth -= 1
      if (depth === 0) {
        const end = i + 1
        try {
          JSON.parse(text.slice(start, end))
          return end
        } catch {
          return null
        }
      }
    }
  }
  return null
}

/** Lexes a known-valid JSON string into colored tokens, appending to `out`. */
function lexJson(json: string, out: HighlightToken[]): void {
  let depth = 0
  let i = 0

  while (i < json.length) {
    const char = json[i]

    if (char === '{' || char === '[') {
      out.push({ text: char, kind: 'bracket', depth })
      depth += 1
      i += 1
    } else if (char === '}' || char === ']') {
      depth = Math.max(0, depth - 1)
      out.push({ text: char, kind: 'bracket', depth })
      i += 1
    } else if (char === '"') {
      const start = i
      i += 1
      while (i < json.length) {
        if (json[i] === '\\') {
          i += 2
          continue
        }
        if (json[i] === '"') {
          i += 1
          break
        }
        i += 1
      }
      // A string is a key when the next non-whitespace character is a colon.
      let after = i
      while (after < json.length && isWhitespace(json[after])) after += 1
      out.push({ text: json.slice(start, i), kind: json[after] === ':' ? 'key' : 'string' })
    } else if (char === ':' || char === ',') {
      out.push({ text: char, kind: 'punct' })
      i += 1
    } else if (isWhitespace(char)) {
      const start = i
      while (i < json.length && isWhitespace(json[i])) i += 1
      out.push({ text: json.slice(start, i), kind: 'plain' })
    } else if (char === '-' || (char >= '0' && char <= '9')) {
      const start = i
      i += 1
      while (i < json.length && isNumberChar(json[i])) i += 1
      out.push({ text: json.slice(start, i), kind: 'number' })
    } else if (matchesAt(json, i, 'true') || matchesAt(json, i, 'false') || matchesAt(json, i, 'null')) {
      const word = matchesAt(json, i, 'true') ? 'true' : matchesAt(json, i, 'false') ? 'false' : 'null'
      out.push({ text: word, kind: 'keyword' })
      i += word.length
    } else {
      // Shouldn't happen for valid JSON, but never drop characters.
      out.push({ text: char, kind: 'plain' })
      i += 1
    }
  }
}

function isWhitespace(char: string): boolean {
  return char === ' ' || char === '\t' || char === '\n' || char === '\r'
}

function isNumberChar(char: string): boolean {
  return (char >= '0' && char <= '9') || char === '.' || char === 'e' || char === 'E' || char === '+' || char === '-'
}

function matchesAt(text: string, index: number, word: string): boolean {
  return text.startsWith(word, index)
}
