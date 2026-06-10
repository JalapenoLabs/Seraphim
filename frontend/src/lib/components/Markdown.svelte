<script lang="ts">
  import { marked } from 'marked'
  import DOMPurify from 'dompurify'

  let { source }: { source: string | null } = $props()

  // Render GitHub-flavored markdown, then sanitize before injecting as HTML so a
  // malicious issue/comment body can't run script. `prose` (Tailwind typography)
  // styles the result; `prose-invert` adapts it to the dark theme.
  const html = $derived(
    DOMPurify.sanitize(marked.parse(source ?? '', { gfm: true, breaks: true }) as string)
  )
</script>

<div
  class="prose prose-sm prose-invert max-w-none break-words font-sans prose-pre:bg-secondary prose-pre:text-foreground prose-code:text-foreground prose-a:text-primary"
>
  {#if source?.trim()}
    {@html html}
  {:else}
    <p class="italic text-muted-foreground">No description provided.</p>
  {/if}
</div>
