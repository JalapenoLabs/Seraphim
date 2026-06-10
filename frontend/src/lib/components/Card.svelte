<script lang="ts">
  import type { Task } from '../types'
  import { goto } from '$app/navigation'
  import { setTaskHold } from '../api'

  let {
    task,
    onchange,
    suggestionCount = 0
  }: { task: Task; onchange: () => void; suggestionCount?: number } = $props()

  async function toggleHold(event: MouseEvent) {
    event.stopPropagation()
    await setTaskHold(task.id, !task.hold)
    onchange()
  }

  function open() {
    goto(`/task/${task.id}`)
  }
</script>

<div
  class="card"
  class:held={task.hold}
  onclick={open}
  onkeydown={(event) => event.key === 'Enter' && open()}
  role="button"
  tabindex="0"
>
  <div class="top">
    <span class="num">#{task.external_id}</span>
    <span class="badge {task.status}">{task.status.replace('_', ' ')}</span>
  </div>
  <div class="title">{task.title}</div>
  {#if suggestionCount > 0}
    <div class="suggestions" title="The agent recommended environment changes">
      💡 {suggestionCount} setup {suggestionCount === 1 ? 'suggestion' : 'suggestions'}
    </div>
  {/if}
  <div class="bottom">
    <button class="hold" title={task.hold ? 'Release hold' : 'Hold (agent skips)'} onclick={toggleHold}>
      {task.hold ? '⏸ held' : 'hold'}
    </button>
    {#if task.pr_url}
      <a href={task.pr_url} target="_blank" rel="noreferrer" onclick={(event) => event.stopPropagation()}>PR</a>
    {/if}
  </div>
  {#if task.error}
    <div class="error">{task.error}</div>
  {/if}
</div>

<style>
  .card {
    background: var(--panel-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.6rem 0.7rem;
    margin-bottom: 0.6rem;
    cursor: grab;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .card:hover {
    border-color: var(--accent);
  }

  .card.held {
    opacity: 0.6;
    border-style: dashed;
  }

  .top {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .num {
    color: var(--muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }

  .title {
    font-size: 0.9rem;
    line-height: 1.3;
  }

  .bottom {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .hold {
    padding: 0.15rem 0.5rem;
    font-size: 0.72rem;
  }

  .error {
    font-size: 0.75rem;
    color: var(--danger);
    border-top: 1px solid var(--border);
    padding-top: 0.35rem;
  }

  /* Loud on purpose: stays bright until the user acknowledges the suggestions. */
  .suggestions {
    background: var(--warn);
    color: #0b1020;
    font-weight: 700;
    font-size: 0.74rem;
    padding: 0.25rem 0.5rem;
    border-radius: 6px;
    text-align: center;
    animation: suggestion-pulse 1.6s ease-in-out infinite;
  }

  @keyframes suggestion-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.55;
    }
  }

  /* Respect users who prefer no motion: stay bright, just don't pulse. */
  @media (prefers-reduced-motion: reduce) {
    .suggestions {
      animation: none;
    }
  }
</style>
