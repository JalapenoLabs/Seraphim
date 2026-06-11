<script lang="ts">
  // The agent's decisions, rendered as part of the issue conversation: resolved
  // ones as quiet history entries, and any open questions gathered into a single
  // wizard (one panel, not a stack of cards). This lives inside the issue feed
  // (the left panel), not in a banner at the top of the task page.
  import type { AnswerSubmission, Question } from '../types'

  import { CircleCheck, CircleSlash } from '@lucide/svelte'

  import QuestionWizard from './QuestionWizard.svelte'

  let {
    questions,
    onSubmit
  }: {
    questions: Question[]
    onSubmit: (answers: AnswerSubmission[]) => void | Promise<void>
  } = $props()

  // History first, the open wizard last, so it reads newest-at-the-bottom like
  // the rest of the conversation.
  const answered = $derived(questions.filter((question) => question.status !== 'pending'))
  const pending = $derived(questions.filter((question) => question.status === 'pending'))

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    })
  }

  function answerSummary(question: Question): string {
    if (question.status === 'declined') {
      return question.answer ? `Declined: ${question.answer}` : 'Skipped'
    }
    return question.answer ?? ''
  }
</script>

{#if questions.length}
  <div class="space-y-4">
    {#each answered as question (question.id)}
      <!-- A resolved decision, shown as a quiet history entry in the feed. -->
      <div class="flex gap-3">
        <div
          class="flex size-10 flex-none items-center justify-center rounded-full border border-border bg-secondary"
        >
          {#if question.status === 'declined'}
            <CircleSlash class="size-5 text-muted-foreground" />
          {:else}
            <CircleCheck class="size-5 text-success" />
          {/if}
        </div>
        <div class="min-w-0 flex-1 rounded-lg border border-border">
          <div
            class="flex items-center gap-2 rounded-t-lg border-b border-border bg-secondary px-4 py-2 text-sm"
          >
            <span class="font-semibold">Seraphim asked</span>
            {#if question.answered_at}
              <span class="text-muted-foreground">· you decided on {formatDate(question.answered_at)}</span>
            {/if}
            <span
              class="ml-auto rounded-full border border-border px-2 py-0.5 text-xs capitalize text-muted-foreground"
            >
              {question.status}
            </span>
          </div>
          <div class="space-y-1.5 px-4 py-3">
            <p class="font-medium">{question.prompt}</p>
            <p class="whitespace-pre-wrap text-sm text-muted-foreground">{answerSummary(question)}</p>
          </div>
        </div>
      </div>
    {/each}

    {#if pending.length}
      <QuestionWizard questions={pending} {onSubmit} />
    {/if}
  </div>
{/if}
