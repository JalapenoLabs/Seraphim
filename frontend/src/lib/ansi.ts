// A minimal SGR (Select Graphic Rendition) parser: turns a string carrying ANSI
// color escapes into styled segments for rendering. CI logs only ever use the
// 8/16-color foreground codes plus bold, so that is all we honor; any other
// escape sequence is dropped so the text stays clean. Dependency-free, in the
// same hand-rolled spirit as `json-highlight.ts`.

export type AnsiSegment = {
  text: string
  classes: string
}

// The escape byte (ESC, 0x1b). Kept as a constant so the regex is built via the
// `RegExp` constructor rather than a literal with a control character in it
// (which the linter flags as `no-control-regex`).
const ESC = '\u001b'
const SGR = new RegExp(`${ESC}\\[([0-9;]*)m`, 'g')

// Foreground color codes mapped to Tailwind text classes. Bright variants
// (90-97) map to slightly lighter shades. White maps to the theme foreground so
// it reads correctly on the dark background rather than as literal white.
const FG_CLASS: Record<number, string> = {
  30: 'text-neutral-500',
  31: 'text-red-400',
  32: 'text-green-400',
  33: 'text-yellow-300',
  34: 'text-blue-400',
  35: 'text-fuchsia-400',
  36: 'text-cyan-300',
  37: 'text-foreground',
  90: 'text-neutral-400',
  91: 'text-red-300',
  92: 'text-green-300',
  93: 'text-yellow-200',
  94: 'text-blue-300',
  95: 'text-fuchsia-300',
  96: 'text-cyan-200',
  97: 'text-foreground'
}

/** Whether the text carries any ANSI escape sequence at all. */
export function hasAnsi(text: string): boolean {
  return text.includes(`${ESC}[`)
}

/**
 * Split `text` into styled segments, applying SGR foreground + bold state as it
 * goes. Text with no escapes returns a single, class-less segment.
 */
export function parseAnsi(text: string): AnsiSegment[] {
  const segments: AnsiSegment[] = []
  let foreground = ''
  let bold = false
  let lastIndex = 0

  function push(chunk: string) {
    if (!chunk) {
      return
    }
    const classes = [foreground, bold ? 'font-bold' : ''].filter(Boolean).join(' ')
    segments.push({ text: chunk, classes })
  }

  SGR.lastIndex = 0
  let match = SGR.exec(text)
  while (match !== null) {
    push(text.slice(lastIndex, match.index))
    lastIndex = SGR.lastIndex

    const codes = match[1] === '' ? [0] : match[1].split(';').map((code) => Number(code))
    for (const code of codes) {
      if (code === 0) {
        foreground = ''
        bold = false
      } else if (code === 1) {
        bold = true
      } else if (code === 22) {
        bold = false
      } else if (code === 39) {
        foreground = ''
      } else if (FG_CLASS[code]) {
        foreground = FG_CLASS[code]
      }
    }

    match = SGR.exec(text)
  }
  push(text.slice(lastIndex))

  return segments
}
