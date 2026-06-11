<script lang="ts">
  // A single panel that walks the user through the agent's clarifying questions
  // one at a time (rather than stacking a card per question): a progress bar,
  // back / skip / next, and a final review step to confirm every answer or jump
  // back and change one before sending them all at once.
  import type { AnswerSubmission, Question } from '../types'

  import { Sparkles } from '@lucide/svelte'

  import { Button } from './ui/button'
  import { Input } from './ui/input'

  let {
    questions,
    onSubmit
  }: {
    // The pending questions, in ask order.
    questions: Question[]
    onSubmit: (answers: AnswerSubmission[]) => void | Promise<void>
  } = $props()

  // `step` indexes the questions; `step === questions.length` is the review page.
  let step = $state(0)
  // Per-question draft answers, keyed by question id. An option choice wins over
  // typed text; `skipped` marks a question the user passed on.
  let optionChoice = $state<Record<string, string>>({})
  let customDraft = $state<Record<string, string>>({})
  let skipped = $state<Record<string, boolean>>({})
  let submitting = $state(false)

  const total = $derived(questions.length)
  const onReview = $derived(step >= total)
  const current = $derived(questions[Math.min(step, Math.max(total - 1, 0))])

  // The resolved answer for a question, or null while it is still untouched.
  function answerFor(id: string): { kind: AnswerSubmission['kind']; text: string } | null {
    if (optionChoice[id]) {
      return { kind: 'option', text: optionChoice[id] }
    }
    const custom = customDraft[id]?.trim()
    if (custom) {
      return { kind: 'custom', text: custom }
    }
    if (skipped[id]) {
      return { kind: 'declined', text: '' }
    }
    return null
  }

  const allResolved = $derived(questions.every((question) => answerFor(question.id) !== null))

  function chooseOption(id: string, title: string) {
    optionChoice[id] = title
    customDraft[id] = ''
    skipped[id] = false
  }

  // Typing a custom answer supersedes any picked option.
  function onCustomInput(id: string) {
    optionChoice[id] = ''
    skipped[id] = false
  }

  function back() {
    step = Math.max(step - 1, 0)
  }

  function next() {
    step = Math.min(step + 1, total)
  }

  function skipCurrent() {
    const id = current.id
    skipped[id] = true
    optionChoice[id] = ''
    customDraft[id] = ''
    next()
  }

  function reviewLabel(question: Question): string {
    const answer = answerFor(question.id)
    if (!answer) {
      return 'Not answered yet'
    }
    return answer.kind === 'declined' ? 'Skipped' : answer.text
  }

  async function send() {
    submitting = true
    try {
      const answers: AnswerSubmission[] = questions.map((question) => {
        const answer = answerFor(question.id) ?? { kind: 'declined' as const, text: '' }
        return { questionId: question.id, kind: answer.kind, text: answer.text }
      })
      await onSubmit(answers)
    } catch (error) {
      console.debug('failed to send answers', error)
      submitting = false
    }
  }
</script>

{#if total > 0}
  <div class="flex gap-3">
    <div
      class="flex size-10 flex-none items-center justify-center rounded-full border border-warning/50 bg-warning/10"
    >
      <Sparkles class="size-5 text-warning" />
    </div>

    <div class="min-w-0 flex-1 rounded-lg border border-warning/50">
      <div
        class="flex items-center justify-between gap-2 rounded-t-lg border-b border-warning/40 bg-warning/10 px-4 py-2 text-sm"
      >
        <span class="font-semibold">Seraphim needs your input</span>
        <span class="text-xs text-muted-foreground">
          {onReview ? 'Review & send' : `Question ${step + 1} of ${total}`}
        </span>
      </div>

      <div class="space-y-4 px-4 py-3">
        <!-- Progress: one segment per question plus a trailing review segment. -->
        <div class="flex items-center gap-1.5">
          {#each questions as question, index (question.id)}
            <button
              type="button"
              aria-label={`Go to question ${index + 1}`}
              onclick={() => (step = index)}
              class="h-1.5 flex-1 rounded-full transition-colors {index < step
                ? 'bg-primary'
                : index === step && !onReview
                  ? 'bg-primary/60'
                  : 'bg-border'}"
            ></button>
          {/each}
          <div class="h-1.5 w-8 rounded-full transition-colors {onReview ? 'bg-primary' : 'bg-border'}"></div>
        </div>

        {#if !onReview}
          <!-- One question at a time. -->
          <p class="font-medium">{current.prompt}</p>

          <div class="flex flex-col gap-2">
            {#each current.options as option}
              <Button
                variant={optionChoice[current.id] === option.title ? 'default' : 'outline'}
                class="h-auto flex-col items-start whitespace-normal py-2 text-left"
                onclick={() => chooseOption(current.id, option.title)}
              >
                <span class="font-semibold">{option.title}</span>
                {#if option.description}
                  <span
                    class="text-xs font-normal {optionChoice[current.id] === option.title
                      ? ''
                      : 'text-muted-foreground'}"
                  >
                    {option.description}
                  </span>
                {/if}
              </Button>
            {/each}
          </div>

          <div>
            <label class="mb-1 block text-xs text-muted-foreground" for={`wizard-custom-${current.id}`}>
              Or type your own answer
            </label>
            <Input
              id={`wizard-custom-${current.id}`}
              placeholder="Type your own answer"
              bind:value={customDraft[current.id]}
              oninput={() => onCustomInput(current.id)}
            />
          </div>

          <div class="flex items-center justify-between gap-2 pt-1">
            <Button variant="ghost" size="sm" disabled={step === 0} onclick={back}>Back</Button>
            <div class="flex gap-2">
              <Button variant="outline" size="sm" onclick={skipCurrent}>Skip</Button>
              <Button size="sm" disabled={answerFor(current.id) === null} onclick={next}>
                {step === total - 1 ? 'Review' : 'Next'}
              </Button>
            </div>
          </div>
        {:else}
          <!-- Final review: confirm each answer, or jump back to change one. -->
          <p class="text-sm text-muted-foreground">Confirm your answers, then send them to the agent.</p>

          <div class="space-y-2">
            {#each questions as question, index (question.id)}
              {@const answer = answerFor(question.id)}
              <div class="rounded-md border border-border p-3">
                <div class="flex items-start justify-between gap-2">
                  <p class="min-w-0 text-sm font-medium">{question.prompt}</p>
                  <Button variant="ghost" size="sm" class="flex-none" onclick={() => (step = index)}>
                    Edit
                  </Button>
                </div>
                <p class="mt-1 whitespace-pre-wrap text-sm {answer ? 'text-muted-foreground' : 'text-warning'}">
                  {reviewLabel(question)}
                </p>
              </div>
            {/each}
          </div>

          <div class="flex items-center justify-between gap-2 pt-1">
            <Button variant="ghost" size="sm" onclick={back}>Back</Button>
            <Button size="sm" disabled={submitting || !allResolved} onclick={send}>
              {submitting ? 'Sending…' : 'Send answers'}
            </Button>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
