<script lang="ts">
  import type { SourceKind } from '../types'

  // Brand marks via unplugin-icons (Simple Icons set), tree-shaken and offline.
  import GithubMark from '~icons/simple-icons/github'
  import JiraMark from '~icons/simple-icons/jira'

  let { source, class: className = '' }: { source: SourceKind; class?: string } = $props()

  // One mark per issue source; add future providers here.
  const MARKS = {
    github: GithubMark,
    jira: JiraMark
  } as const satisfies Record<SourceKind, typeof GithubMark>

  const Mark = $derived(MARKS[source])
</script>

<Mark class={className} />
