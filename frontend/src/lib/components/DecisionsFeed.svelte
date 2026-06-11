<script lang="ts">
  // The agent's decisions, rendered as part of the issue conversation: resolved
  // ones as quiet history entries, the open one as an active prompt. This lives
  // inside the issue feed (the left panel), not in a banner at the top of the
  // task page, so a decision reads like another message in the thread.
  import type { AnswerKind, Question } from '../types'

  import { Sparkles, CircleCheck, CircleSlash } from '@lucide/svelte'

  import { Button } from './ui/button'
  import { Input } from './ui/input'

  let {
    questions,
    onAnswer
  }: {
    questions: Question[]
    onAnswer: (questionId: string, kind: AnswerKind, text: string) => void | Promise<void>
  } = $props()

  // Free text for the "something else" / "decline" choices, keyed by question id.
  let customText = $state<Record<string, string>>({})
  let declineText = $state<Record<string, string>>({})

  // History first, the open question last, so it reads newest-at-the-bottom like
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
      return question.answer ? `Declined: ${question.answer}` : 'Declined to choose'
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

    {#each pending as question (question.id)}
      <!-- The active decision: prominent, but still inside the conversation feed. -->
      <div class="flex gap-3">
        <div
          class="flex size-10 flex-none items-center justify-center rounded-full border border-warning/50 bg-warning/10"
        >
          <Sparkles class="size-5 text-warning" />
        </div>
        <div class="min-w-0 flex-1 rounded-lg border border-warning/50">
          <div
            class="flex items-center gap-2 rounded-t-lg border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm font-semibold"
          >
            Seraphim needs your input
          </div>
          <div class="space-y-3 px-4 py-3">
            <p class="font-medium">{question.prompt}</p>

            <div class="flex flex-col gap-2">
              {#each question.options as option}
                <Button
                  variant="outline"
                  class="h-auto flex-col items-start whitespace-normal py-2 text-left"
                  onclick={() => onAnswer(question.id, 'option', option.title)}
                >
                  <span class="font-semibold">{option.title}</span>
                  {#if option.description}
                    <span class="text-xs font-normal text-muted-foreground">{option.description}</span>
                  {/if}
                </Button>
              {/each}
            </div>

            <div>
              <label class="mb-1 block text-xs text-muted-foreground" for={`decision-custom-${question.id}`}>
                Something else
              </label>
              <div class="flex gap-2">
                <Input
                  id={`decision-custom-${question.id}`}
                  placeholder="Type your own answer"
                  bind:value={customText[question.id]}
                />
                <Button
                  disabled={!customText[question.id]?.trim()}
                  onclick={() => onAnswer(question.id, 'custom', customText[question.id]?.trim() ?? '')}
                >
                  Send
                </Button>
              </div>
            </div>

            <div>
              <label class="mb-1 block text-xs text-muted-foreground" for={`decision-decline-${question.id}`}>
                Decline and chat about this
              </label>
              <div class="flex gap-2">
                <Input
                  id={`decision-decline-${question.id}`}
                  placeholder="Optional note for the agent"
                  bind:value={declineText[question.id]}
                />
                <Button
                  variant="secondary"
                  onclick={() => onAnswer(question.id, 'declined', declineText[question.id]?.trim() ?? '')}
                >
                  Decline
                </Button>
              </div>
            </div>
          </div>
        </div>
      </div>
    {/each}
  </div>
{/if}
