<script lang="ts">
  import type { DiffLine, DiffLineKind } from '$lib/diff'

  // `collapsed` hides the patch rows but keeps the one-line summary, so a caller
  // can offer an expand/collapse affordance for big, all-additions writes.
  let {
    lines,
    added,
    removed,
    collapsed = false
  }: { lines: DiffLine[]; added: number; removed: number; collapsed?: boolean } = $props()

  // A long Write would otherwise flood the log; show a generous window and note
  // the remainder rather than rendering hundreds of rows.
  const MAX_ROWS = 200
  const shown = $derived(lines.slice(0, MAX_ROWS))
  const overflow = $derived(Math.max(0, lines.length - MAX_ROWS))

  const SIGN: Record<DiffLineKind, string> = { add: '+', del: '-', context: ' ' }

  // The whole row is washed in a subtle red/green; the sign gutter carries the
  // same hue so the change reads at a glance even on a wrapped line.
  const ROW_CLASS: Record<DiffLineKind, string> = {
    add: 'bg-success/15',
    del: 'bg-destructive/15',
    context: ''
  }
  const GUTTER_CLASS: Record<DiffLineKind, string> = {
    add: 'text-success',
    del: 'text-destructive',
    context: 'text-muted-foreground/50'
  }

  function plural(count: number, word: string): string {
    return `${count} ${word}${count === 1 ? '' : 's'}`
  }
</script>

<div class="min-w-0 flex-1">
  <div class="text-xs text-muted-foreground">
    Added {plural(added, 'line')}, removed {plural(removed, 'line')}
  </div>

  {#if shown.length && !collapsed}
    <div class="mt-1 overflow-hidden rounded-md border border-border text-xs">
      {#each shown as line}
        <div class="flex {ROW_CLASS[line.kind]}">
          <span class="w-[2ch] flex-none select-none text-center {GUTTER_CLASS[line.kind]}">
            {SIGN[line.kind]}
          </span>
          <span
            class="min-w-0 flex-1 whitespace-pre-wrap break-words pr-2 {line.kind === 'context'
              ? 'text-muted-foreground'
              : 'text-foreground'}"
          >
            {line.text || ' '}
          </span>
        </div>
      {/each}
      {#if overflow > 0}
        <div class="px-2 py-1 text-muted-foreground">… {plural(overflow, 'more line')} not shown</div>
      {/if}
    </div>
  {/if}
</div>
