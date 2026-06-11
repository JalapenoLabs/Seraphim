<script lang="ts">
  import type { AnswerKind, IssueThread, IssueUser, Question, Task } from '../types'

  import { onMount } from 'svelte'
  import { toast } from 'svelte-sonner'
  import {
    CircleDot,
    CircleCheck,
    CircleSlash,
    ChevronDown,
    GitPullRequest,
    GitBranch,
    ExternalLink,
    Plus,
    Link
  } from '@lucide/svelte'

  import { addIssueComment, getIssueThread, setIssueState } from '../api'
  import Markdown from './Markdown.svelte'
  import SourceIcon from './SourceIcon.svelte'
  import DecisionsFeed from './DecisionsFeed.svelte'
  import { Button, buttonVariants } from './ui/button'
  import { Textarea } from './ui/textarea'
  import * as DropdownMenu from './ui/dropdown-menu'

  let {
    task,
    questions,
    onAnswer
  }: {
    task: Task
    // The agent's escalated decisions, rendered inline in the conversation feed.
    questions: Question[]
    onAnswer: (questionId: string, kind: AnswerKind, text: string) => void | Promise<void>
  } = $props()

  // GitHub URLs derived from the issue link: the repo root and its "new issue"
  // chooser, so the header can link out without the repo full name on hand.
  const repoUrl = $derived(task.url ? task.url.replace(/\/issues\/\d+.*$/, '') : '')
  const newIssueUrl = $derived(repoUrl ? `${repoUrl}/issues/new/choose` : '')

  async function copyLink() {
    if (!task.url) {
      return
    }
    try {
      await navigator.clipboard.writeText(task.url)
      toast.success('Link copied to clipboard')
    } catch {
      toast.error('Could not copy the link')
    }
  }

  let thread = $state<IssueThread | null>(null)
  let loadError = $state<string | null>(null)
  let commentBody = $state('')
  let posting = $state(false)

  const isGithub = $derived(task.source_kind === 'github')

  // Everyone who appears in the thread, de-duplicated, for the Participants list.
  const participants = $derived.by(() => {
    if (!thread) {
      return [] as IssueUser[]
    }
    const seen = new Map<string, IssueUser>()
    for (const person of [thread.issue.user, ...thread.comments.map((comment) => comment.user)]) {
      if (!seen.has(person.login)) {
        seen.set(person.login, person)
      }
    }
    return [...seen.values()]
  })

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    })
  }

  // GitHub only badges associations that signal authority; "NONE" stays quiet.
  function association(value: string): string | null {
    if (!value || value === 'NONE') {
      return null
    }
    return value.charAt(0) + value.slice(1).toLowerCase()
  }

  async function load() {
    if (!isGithub) {
      return
    }
    try {
      thread = await getIssueThread(task.id)
    } catch (error) {
      loadError = error instanceof Error ? error.message : 'Failed to load the issue from GitHub.'
    }
  }

  async function submitComment() {
    if (!commentBody.trim() || !thread) {
      return
    }
    posting = true
    try {
      const comment = await addIssueComment(task.id, commentBody)
      thread = { ...thread, comments: [...thread.comments, comment] }
      commentBody = ''
      toast.success('Comment added')
    } catch {
      toast.error('Could not post the comment')
    } finally {
      posting = false
    }
  }

  // Open or close the issue, optionally posting a pending comment first (GitHub's
  // "close with comment"). `reason` is GitHub's close reason.
  let updatingState = $state(false)
  async function changeState(state: 'open' | 'closed', reason?: 'completed' | 'not_planned') {
    if (!thread) {
      return
    }
    updatingState = true
    try {
      if (commentBody.trim()) {
        await submitComment()
      }
      const issue = await setIssueState(task.id, state, reason)
      thread = { ...thread, issue }
      toast.success(state === 'closed' ? 'Issue closed' : 'Issue reopened')
    } catch {
      toast.error(state === 'closed' ? 'Could not close the issue' : 'Could not reopen the issue')
    } finally {
      updatingState = false
    }
  }

  // GitHub relabels the close/reopen button when there's a pending comment.
  const hasComment = $derived(commentBody.trim().length > 0)

  onMount(load)
</script>

{#snippet commentCard(user: IssueUser, createdAt: string, assoc: string, body: string | null)}
  <div class="flex gap-3">
    <img src={user.avatar_url} alt={user.login} class="size-10 flex-none rounded-full" />
    <div class="min-w-0 flex-1 rounded-lg border border-border">
      <div class="flex items-center gap-2 rounded-t-lg border-b border-border bg-secondary px-4 py-2 text-sm">
        <a href={user.html_url} target="_blank" rel="noreferrer" class="font-semibold hover:text-primary">
          {user.login}
        </a>
        <span class="text-muted-foreground">commented on {formatDate(createdAt)}</span>
        {#if association(assoc)}
          <span class="ml-auto rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground">
            {association(assoc)}
          </span>
        {/if}
      </div>
      <div class="px-4 py-3">
        <Markdown source={body} />
      </div>
    </div>
  </div>
{/snippet}

<div class="flex h-full flex-col">
  <!-- Issue header -->
  <header class="flex-none border-b border-border pb-4">
    <div class="flex items-start justify-between gap-4">
      <h1 class="flex min-w-0 items-center gap-2 text-2xl font-semibold leading-tight">
        <SourceIcon source={task.source_kind} class="size-5 flex-none text-muted-foreground" />
        <span class="min-w-0">
          {(thread?.issue.title ?? task.title)}
          <span class="font-normal text-muted-foreground">#{task.external_id}</span>
        </span>
      </h1>
      <div class="flex flex-none items-center gap-2">
        {#if task.url}
          <a href={task.url} target="_blank" rel="noreferrer" class={buttonVariants({ variant: 'outline', size: 'sm' })}>
            <ExternalLink class="size-4" /> Open
          </a>
        {/if}
        {#if isGithub && newIssueUrl}
          <a href={newIssueUrl} target="_blank" rel="noreferrer" class={buttonVariants({ variant: 'outline', size: 'sm' })}>
            <Plus class="size-4" /> New issue
          </a>
        {/if}
        {#if task.url}
          <Button variant="outline" size="sm" onclick={copyLink}>
            <Link class="size-4" /> Copy link
          </Button>
        {/if}
      </div>
    </div>
    {#if thread}
      <div class="mt-3 flex flex-wrap items-center gap-3">
        {#if thread.issue.state === 'open'}
          <span class="inline-flex items-center gap-1.5 rounded-full bg-success px-3 py-1 text-sm font-medium text-success-foreground">
            <CircleDot class="size-4" /> Open
          </span>
        {:else}
          <span class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-sm font-medium text-white" style="background:#8957e5">
            <CircleCheck class="size-4" /> Closed
          </span>
        {/if}
        <span class="text-sm text-muted-foreground">
          <span class="font-semibold text-foreground">{thread.issue.user.login}</span>
          opened this on {formatDate(thread.issue.created_at)} · {thread.comments.length}
          {thread.comments.length === 1 ? 'comment' : 'comments'}
        </span>
      </div>
    {/if}
  </header>

  <!-- Conversation + sidebar -->
  <div class="flex min-h-0 flex-1 gap-6 overflow-y-auto pt-4">
    <div class="min-w-0 flex-1 space-y-4">
      {#if loadError}
        <div class="rounded-lg border border-destructive/40 bg-destructive/10 p-3 text-sm text-destructive">
          {loadError}
        </div>
      {/if}

      {#if thread}
        {@render commentCard(
          thread.issue.user,
          thread.issue.created_at,
          thread.issue.author_association,
          thread.issue.body
        )}
        {#each thread.comments as comment}
          {@render commentCard(comment.user, comment.created_at, comment.author_association, comment.body)}
        {/each}

        <!-- The agent's decisions, inline in the feed below the conversation. -->
        <DecisionsFeed {questions} {onAnswer} />

        <!-- Add a comment / change state -->
        <div class="rounded-lg border border-border p-3">
          <Textarea
            rows={4}
            placeholder="Leave a comment"
            bind:value={commentBody}
            class="resize-y font-mono text-sm"
          />
          <div class="mt-2 flex flex-wrap items-center justify-end gap-2">
            {#if thread.issue.state === 'open'}
              <!-- Split button: close (completed) + a dropdown of close reasons. -->
              <div class="flex">
                <Button
                  variant="outline"
                  class="rounded-r-none"
                  disabled={updatingState}
                  onclick={() => changeState('closed', 'completed')}
                >
                  <CircleCheck class="size-4" />
                  {hasComment ? 'Close with comment' : 'Close issue'}
                </Button>
                <DropdownMenu.Root>
                  <DropdownMenu.Trigger
                    class={`${buttonVariants({ variant: 'outline' })} rounded-l-none border-l-0 px-2`}
                    aria-label="Close reasons"
                    disabled={updatingState}
                  >
                    <ChevronDown class="size-4" />
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content align="end">
                    <DropdownMenu.Item onclick={() => changeState('closed', 'completed')}>
                      <CircleCheck class="size-4 text-success" />
                      <div>
                        <div>Close as completed</div>
                        <div class="text-xs text-muted-foreground">Done, closed, fully completed</div>
                      </div>
                    </DropdownMenu.Item>
                    <DropdownMenu.Item onclick={() => changeState('closed', 'not_planned')}>
                      <CircleSlash class="size-4 text-muted-foreground" />
                      <div>
                        <div>Close as not planned</div>
                        <div class="text-xs text-muted-foreground">Won't fix, can't repro, duplicate, stale</div>
                      </div>
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </div>
            {:else}
              <Button variant="outline" disabled={updatingState} onclick={() => changeState('open')}>
                <CircleDot class="size-4" />
                {hasComment ? 'Reopen with comment' : 'Reopen issue'}
              </Button>
            {/if}
            <Button disabled={posting || !commentBody.trim()} onclick={submitComment}>
              {posting ? 'Commenting…' : 'Comment'}
            </Button>
          </div>
        </div>
      {:else if !isGithub}
        <!-- Non-GitHub source: render what the card holds, GitHub-styled. -->
        <div class="rounded-lg border border-border p-4">
          <Markdown source={task.body_snapshot} />
        </div>
        <DecisionsFeed {questions} {onAnswer} />
      {:else if !loadError}
        <p class="text-sm text-muted-foreground">Loading issue…</p>
      {/if}
    </div>

    <!-- Sidebar -->
    {#if thread}
      <aside class="hidden w-56 flex-none space-y-5 text-sm lg:block">
        <section>
          <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Assignees</h3>
          {#if thread.issue.assignees.length}
            {#each thread.issue.assignees as assignee}
              <a href={assignee.html_url} target="_blank" rel="noreferrer" class="mb-1 flex items-center gap-2 hover:text-primary">
                <img src={assignee.avatar_url} alt={assignee.login} class="size-5 rounded-full" />
                {assignee.login}
              </a>
            {/each}
          {:else}
            <p class="text-muted-foreground">No one assigned</p>
          {/if}
        </section>

        <hr class="border-border" />

        <section>
          <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Labels</h3>
          {#if thread.issue.labels.length}
            <div class="flex flex-wrap gap-1.5">
              {#each thread.issue.labels as label}
                <span
                  class="rounded-full border px-2 py-0.5 text-xs font-medium"
                  style="border-color:#{label.color};color:#{label.color}"
                >
                  {label.name}
                </span>
              {/each}
            </div>
          {:else}
            <p class="text-muted-foreground">None yet</p>
          {/if}
        </section>

        <hr class="border-border" />

        <section>
          <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Development</h3>
          {#if task.pr_url}
            <a href={task.pr_url} target="_blank" rel="noreferrer" class="flex items-center gap-2 text-primary hover:underline">
              <GitPullRequest class="size-4 flex-none" /> Pull request
            </a>
          {/if}
          {#if task.branch}
            <div class="mt-1 flex items-center gap-2 text-muted-foreground">
              <GitBranch class="size-4 flex-none" />
              <span class="break-all font-mono text-xs">{task.branch}</span>
            </div>
          {/if}
          {#if !task.pr_url && !task.branch}
            <p class="text-muted-foreground">Nothing yet</p>
          {/if}
        </section>

        {#if thread.issue.milestone}
          <hr class="border-border" />
          <section>
            <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Milestone</h3>
            <p>{thread.issue.milestone.title}</p>
          </section>
        {/if}

        <hr class="border-border" />

        <section>
          <h3 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {participants.length} Participant{participants.length === 1 ? '' : 's'}
          </h3>
          <div class="flex flex-wrap gap-1.5">
            {#each participants as person}
              <a href={person.html_url} target="_blank" rel="noreferrer" title={person.login}>
                <img src={person.avatar_url} alt={person.login} class="size-7 rounded-full" />
              </a>
            {/each}
          </div>
        </section>
      </aside>
    {/if}
  </div>
</div>
