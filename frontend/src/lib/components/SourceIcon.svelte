<script lang="ts">
  import type { SourceKind } from '../types'

  // Brand marks via unplugin-icons (Simple Icons set), tree-shaken and offline.
  import GithubMark from '~icons/simple-icons/github'
  import JiraMark from '~icons/simple-icons/jira'
  // Internal tickets have no brand; a database glyph marks "our DB only".
  import InternalMark from '~icons/lucide/database'

  let { source, class: className = '' }: { source: SourceKind; class?: string } = $props()

  // One mark per issue source; add future providers here.
  const MARKS = {
    github: GithubMark,
    jira: JiraMark,
    internal: InternalMark
  } as const satisfies Record<SourceKind, typeof GithubMark>

  const Mark = $derived(MARKS[source])
</script>

<Mark class={className} />
