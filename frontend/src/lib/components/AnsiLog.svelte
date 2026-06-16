<!--
  Renders a snippet of log text, honoring any ANSI color it carries. When the
  text has no ANSI color of its own and it represents an error, the whole block
  falls back to red so a failure still reads as a failure (issue #185).
-->
<script lang="ts">
  import { hasAnsi, parseAnsi } from '$lib/ansi'

  type Props = {
    text: string
    /** Tint the whole block red when the log carries no color of its own. */
    isError?: boolean
  }

  let { text, isError = false }: Props = $props()

  const colored = $derived(hasAnsi(text))
  const segments = $derived(parseAnsi(text))
  // Plain (color-less) error logs get a red base; everything else stays muted
  // and lets per-segment ANSI colors show through.
  const baseColor = $derived(!colored && isError ? 'text-destructive' : 'text-muted-foreground')
</script>

<pre
  class="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded-md bg-background/60 px-2 py-1.5 font-mono text-xs leading-snug {baseColor}"
>{#each segments as segment}<span class={segment.classes}>{segment.text}</span>{/each}</pre>
