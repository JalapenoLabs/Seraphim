<script lang="ts">
  // The Automation page: create rulesets that react to issue events (created /
  // updated / commented) and act on a match (today: move the card to To Do).
  // Each rule is an inline, always-open editor; saving creates or updates it.
  import type {
    AutomationRule,
    AutomationTrigger,
    QueuePosition,
    RuleCombinator,
    RuleCondition,
    RuleField,
    RuleOperator,
    RuleSource
  } from '$lib/types'
  import type { RuleRequest } from '$lib/api'

  import { onMount } from 'svelte'
  import { Info, Plus, Trash2, X, Zap } from '@lucide/svelte'

  import {
    createAutomationRule,
    deleteAutomationRule,
    getSettings,
    listAutomationRules,
    updateAutomationRule
  } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'
  import { Label } from '$lib/components/ui/label'

  // A condition while editing: values are kept as free text and parsed on save.
  type ConditionDraft = { field: RuleField; operator: RuleOperator; valuesText: string }
  type RuleDraft = {
    id?: string
    name: string
    enabled: boolean
    source_kind: RuleSource
    triggers: AutomationTrigger[]
    combinator: RuleCombinator
    conditions: ConditionDraft[]
    actionPosition: QueuePosition
    saving: boolean
    savedAt: string | null
    error: string | null
  }

  let rules = $state<RuleDraft[]>([])
  let loading = $state(true)

  // Whether a GitHub webhook secret is configured. Without one, `Updated` /
  // `Comment` rules never fire (only the realtime webhook path evaluates them);
  // `Created` rules still fire from the poll sync (issue #229). Drives the notice
  // below so an enabled rule never sits silently inert.
  let githubWebhookConfigured = $state(true)
  let webhookNoticeDismissed = $state(false)
  const hasEnabledRules = $derived(rules.some((rule) => rule.enabled))
  const showWebhookNotice = $derived(
    hasEnabledRules && !githubWebhookConfigured && !webhookNoticeDismissed
  )

  const FIELDS: { value: RuleField; label: string; placeholder: string; list: boolean }[] = [
    { value: 'labels', label: 'Labels', placeholder: 'automation, bug', list: true },
    { value: 'author', label: 'Author', placeholder: 'navarrotech', list: false },
    { value: 'repo', label: 'Repository', placeholder: 'owner/repo', list: false },
    { value: 'title', label: 'Title', placeholder: 'crash, regression', list: false },
    { value: 'body', label: 'Body', placeholder: 'urgent', list: false },
    { value: 'comment', label: 'Comment', placeholder: 'Jarvis, can you take this on?', list: false },
    { value: 'comment_author', label: 'Comment author', placeholder: 'navarrotech', list: false },
    { value: 'state', label: 'State', placeholder: 'open', list: false }
  ]
  const OPERATORS: { value: RuleOperator; label: string; needsValues: boolean }[] = [
    { value: 'has_one_of', label: 'has one of', needsValues: true },
    { value: 'exactly', label: 'is exactly', needsValues: true },
    { value: 'contains', label: 'contains', needsValues: true },
    { value: 'is_empty', label: 'is empty', needsValues: false },
    { value: 'is_not_empty', label: 'is not empty', needsValues: false }
  ]
  const TRIGGERS: { value: AutomationTrigger; label: string }[] = [
    { value: 'created', label: 'Created' },
    { value: 'updated', label: 'Updated' },
    { value: 'comment', label: 'Comment' }
  ]
  const SOURCES: { value: RuleSource; label: string }[] = [
    { value: 'github', label: 'GitHub' },
    { value: 'jira', label: 'Jira' },
    { value: 'any', label: 'Any source' }
  ]

  const SELECT_CLASS =
    'h-9 rounded-md border border-border bg-background px-2 text-sm text-foreground focus:border-primary focus:outline-none'

  function operatorNeedsValues(operator: RuleOperator): boolean {
    return OPERATORS.find((entry) => entry.value === operator)?.needsValues ?? true
  }
  function fieldMeta(field: RuleField) {
    return FIELDS.find((entry) => entry.value === field) ?? FIELDS[0]
  }

  function toDraft(rule: AutomationRule): RuleDraft {
    return {
      id: rule.id,
      name: rule.name,
      enabled: rule.enabled,
      source_kind: rule.source_kind,
      triggers: [...rule.triggers],
      combinator: rule.criteria.combinator,
      conditions: rule.criteria.conditions.map((condition) => ({
        field: condition.field,
        operator: condition.operator,
        valuesText: condition.values.join(', ')
      })),
      actionPosition: rule.action.position,
      saving: false,
      savedAt: null,
      error: null
    }
  }

  function blankDraft(): RuleDraft {
    return {
      name: '',
      enabled: true,
      source_kind: 'github',
      triggers: ['created'],
      combinator: 'and',
      conditions: [{ field: 'labels', operator: 'has_one_of', valuesText: '' }],
      actionPosition: 'top',
      saving: false,
      savedAt: null,
      error: null
    }
  }

  function toRequest(draft: RuleDraft): RuleRequest {
    const conditions: RuleCondition[] = draft.conditions.map((condition) => ({
      field: condition.field,
      operator: condition.operator,
      values: operatorNeedsValues(condition.operator)
        ? condition.valuesText
            .split(',')
            .map((value) => value.trim())
            .filter((value) => value.length > 0)
        : []
    }))
    return {
      name: draft.name.trim() || 'Untitled rule',
      enabled: draft.enabled,
      source_kind: draft.source_kind,
      triggers: draft.triggers,
      criteria: { combinator: draft.combinator, conditions },
      action: { type: 'move_to_todo', position: draft.actionPosition }
    }
  }

  async function load() {
    loading = true
    try {
      const [fetched, settings] = await Promise.all([listAutomationRules(), getSettings()])
      rules = fetched.map(toDraft)
      githubWebhookConfigured = settings.github_webhook_secret_set
    } finally {
      loading = false
    }
  }

  onMount(load)

  function addRule() {
    rules = [...rules, blankDraft()]
  }

  function addCondition(draft: RuleDraft) {
    draft.conditions = [...draft.conditions, { field: 'labels', operator: 'has_one_of', valuesText: '' }]
  }
  function removeCondition(draft: RuleDraft, index: number) {
    draft.conditions = draft.conditions.filter((_, i) => i !== index)
  }
  function toggleTrigger(draft: RuleDraft, trigger: AutomationTrigger) {
    draft.triggers = draft.triggers.includes(trigger)
      ? draft.triggers.filter((value) => value !== trigger)
      : [...draft.triggers, trigger]
  }

  async function save(draft: RuleDraft) {
    draft.saving = true
    draft.error = null
    try {
      const body = toRequest(draft)
      const saved = draft.id
        ? await updateAutomationRule(draft.id, body)
        : await createAutomationRule(body)
      draft.id = saved.id
      draft.savedAt = new Date().toLocaleTimeString()
    } catch (error) {
      console.error('failed to save rule', error)
      draft.error = 'Failed to save. Check the rule and try again.'
    } finally {
      draft.saving = false
    }
  }

  async function remove(draft: RuleDraft) {
    if (draft.id) {
      try {
        await deleteAutomationRule(draft.id)
      } catch (error) {
        console.error('failed to delete rule', error)
        return
      }
    }
    rules = rules.filter((entry) => entry !== draft)
  }
</script>

<div class="mx-auto max-w-4xl space-y-6 p-6">
  <header class="flex items-start justify-between gap-4">
    <div>
      <h1 class="flex items-center gap-2 text-2xl font-bold tracking-tight">
        <Zap class="size-6 text-primary" /> Automation
      </h1>
      <p class="mt-1 max-w-2xl text-sm text-muted-foreground">
        Rules react to issue events and, on a match, run their action (for example moving the card to
        the top of To Do). Try: <em>labels has one of "automation, bug" AND author is exactly
        navarrotech</em>, or a comment that contains "Jarvis, can you take this on?".
        <strong>Created</strong> rules fire as issues sync onto the board, with or without a webhook;
        <strong>Updated</strong> and <strong>Comment</strong> rules fire only from a configured GitHub
        webhook.
      </p>
    </div>
    <Button onclick={addRule}><Plus class="size-4" /> New rule</Button>
  </header>

  {#if showWebhookNotice}
    <!-- Don't fail silently (issue #229): an enabled rule with no webhook secret
         would leave Updated/Comment triggers inert. Created still works via the
         poll, so this is informational, not alarming, and dismissible. -->
    <div class="flex items-start gap-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4 text-sm">
      <Info class="mt-0.5 size-5 flex-none text-amber-500" />
      <div class="min-w-0 flex-1 space-y-1">
        <p class="font-medium">No GitHub webhook is configured.</p>
        <p class="text-muted-foreground">
          <strong>Created</strong> rules still fire as issues sync onto the board, so they work now.
          But <strong>Updated</strong> and <strong>Comment</strong> rules need a webhook: set a GitHub
          webhook secret in <a class="underline" href="/settings">Settings</a>, then add a repo (or
          org) webhook for the <em>Issues</em> and <em>Issue comments</em> events with that secret,
          pointing at <code class="rounded bg-muted px-1">POST /api/v1/webhooks/github</code> on this
          instance's Tailscale URL.
        </p>
      </div>
      <button
        type="button"
        onclick={() => (webhookNoticeDismissed = true)}
        title="Dismiss"
        aria-label="Dismiss notice"
        class="rounded-md p-1 text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground"
      >
        <X class="size-4" />
      </button>
    </div>
  {/if}

  {#if loading}
    <p class="text-muted-foreground">Loading…</p>
  {:else if rules.length === 0}
    <Card.Root>
      <Card.Content class="py-10 text-center text-muted-foreground">
        No rules yet. Click <span class="font-medium text-foreground">New rule</span> to create one.
      </Card.Content>
    </Card.Root>
  {/if}

  {#each rules as draft (draft)}
    <Card.Root class={draft.enabled ? '' : 'opacity-70'}>
      <Card.Content class="space-y-5 pt-6">
        <!-- Name + enabled + source -->
        <div class="flex flex-wrap items-center gap-3">
          <Switch bind:checked={draft.enabled} aria-label="Enabled" />
          <Input class="max-w-xs flex-1" placeholder="Rule name" bind:value={draft.name} />
          <select class={SELECT_CLASS} bind:value={draft.source_kind} aria-label="Source">
            {#each SOURCES as source}
              <option value={source.value}>{source.label}</option>
            {/each}
          </select>
          <button
            type="button"
            onclick={() => remove(draft)}
            title="Delete rule"
            aria-label="Delete rule"
            class="ml-auto rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
          >
            <Trash2 class="size-4" />
          </button>
        </div>

        <!-- Triggers -->
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">On</span>
          {#each TRIGGERS as trigger}
            <button
              type="button"
              onclick={() => toggleTrigger(draft, trigger.value)}
              aria-pressed={draft.triggers.includes(trigger.value)}
              class="rounded-full border px-3 py-1 text-xs font-medium transition-colors {draft.triggers.includes(
                trigger.value
              )
                ? 'border-primary bg-primary/15 text-primary'
                : 'border-border text-muted-foreground hover:bg-secondary'}"
            >
              {trigger.label}
            </button>
          {/each}
        </div>

        <!-- Conditions -->
        <div class="space-y-2 rounded-lg border border-border p-3">
          <div class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Match
            <select class={SELECT_CLASS} bind:value={draft.combinator} aria-label="Combinator">
              <option value="and">all (AND)</option>
              <option value="or">any (OR)</option>
            </select>
            of:
          </div>

          {#each draft.conditions as condition, index (index)}
            {@const meta = fieldMeta(condition.field)}
            <div class="flex flex-wrap items-center gap-2">
              <select class={SELECT_CLASS} bind:value={condition.field} aria-label="Field">
                {#each FIELDS as field}
                  <option value={field.value}>{field.label}</option>
                {/each}
              </select>
              <select class={SELECT_CLASS} bind:value={condition.operator} aria-label="Operator">
                {#each OPERATORS as operator}
                  <option value={operator.value}>{operator.label}</option>
                {/each}
              </select>
              {#if operatorNeedsValues(condition.operator)}
                <Input
                  class="min-w-0 flex-1"
                  placeholder={meta.list ? `${meta.placeholder} (comma-separated)` : meta.placeholder}
                  bind:value={condition.valuesText}
                />
              {:else}
                <span class="flex-1 text-sm text-muted-foreground">(no value needed)</span>
              {/if}
              <button
                type="button"
                onclick={() => removeCondition(draft, index)}
                title="Remove condition"
                aria-label="Remove condition"
                class="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-secondary hover:text-foreground"
              >
                <Trash2 class="size-4" />
              </button>
            </div>
          {/each}

          <Button variant="outline" size="sm" onclick={() => addCondition(draft)}>
            <Plus class="size-4" /> Add condition
          </Button>
        </div>

        <!-- Action -->
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-xs font-semibold uppercase tracking-wide text-muted-foreground">Then</span>
          <span class="text-sm">move the card to</span>
          <select class={SELECT_CLASS} bind:value={draft.actionPosition} aria-label="Queue position">
            <option value="top">the top</option>
            <option value="bottom">the bottom</option>
          </select>
          <span class="text-sm">of To Do.</span>
        </div>

        <!-- Save -->
        <div class="flex items-center gap-3 border-t border-border pt-4">
          <Button onclick={() => save(draft)} disabled={draft.saving}>
            {draft.saving ? 'Saving…' : draft.id ? 'Save' : 'Create rule'}
          </Button>
          {#if draft.error}
            <span class="text-sm text-destructive">{draft.error}</span>
          {:else if draft.savedAt}
            <span class="text-sm text-muted-foreground">Saved at {draft.savedAt}</span>
          {/if}
          {#if !draft.triggers.length}
            <span class="text-sm text-warning">Pick at least one trigger.</span>
          {/if}
        </div>
      </Card.Content>
    </Card.Root>
  {/each}
</div>
