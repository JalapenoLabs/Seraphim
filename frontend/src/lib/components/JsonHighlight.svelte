<script lang="ts">
  import type { HighlightToken } from '$lib/json-highlight'
  import { highlightJson } from '$lib/json-highlight'

  let { text }: { text: string } = $props()

  const tokens = $derived(highlightJson(text))

  // Rainbow-by-depth so nested bracket groups are easy to pair up by eye.
  const BRACKET_COLORS = ['text-amber-300', 'text-fuchsia-400', 'text-sky-400']

  // Plain text returns '' so it inherits the surrounding line's color.
  function colorFor(token: HighlightToken): string {
    switch (token.kind) {
      case 'bracket':
        return BRACKET_COLORS[(token.depth ?? 0) % BRACKET_COLORS.length]
      case 'key':
        return 'text-cyan-300'
      case 'string':
        return 'text-emerald-300'
      case 'number':
        return 'text-orange-300'
      case 'keyword':
        return 'text-violet-300'
      case 'punct':
        return 'text-muted-foreground'
      default:
        return ''
    }
  }
</script>

{#each tokens as token}<span class={colorFor(token)}>{token.text}</span>{/each}
