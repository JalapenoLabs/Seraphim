<script lang="ts">
  // The Railways management page. Railways are the parallel agent lanes: each has
  // its own workspace container, agent loop, Claude session, and set of repos. A
  // repository belongs to exactly one railway, so a task's lane always follows its
  // repo. The `main` railway holds everything by default and cannot be deleted or
  // stopped. This page is where the operator creates lanes, renames them, pauses a
  // lane independently of the global master pause, starts or stops a lane's
  // container, deletes a lane, and assigns repos between lanes.
  import type { Railway, RailwayState, Repository, Settings } from '$lib/types'
  import type { DndEvent } from 'svelte-dnd-action'

  import { onMount } from 'svelte'
  import { toast } from 'svelte-sonner'
  import { dndzone } from 'svelte-dnd-action'

  import {
    assignRepoToRailway,
    createRailway,
    deleteRailway,
    extractApiError,
    getSettings,
    listRailways,
    listRepos,
    setPaused,
    setRailwayPaused,
    startRailway,
    stopRailway,
    updateRailway,
    updateSettings
  } from '$lib/api'
  import * as Card from '$lib/components/ui/card'
  import * as AlertDialog from '$lib/components/ui/alert-dialog'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Label } from '$lib/components/ui/label'
  import { Badge } from '$lib/components/ui/badge'
  import { Switch } from '$lib/components/ui/switch'

  let settings = $state<Settings | null>(null)
  let repos = $state<Repository[]>([])
  let railways = $state<Railway[]>([])

  // Repos grouped into one drag-and-drop bucket per railway, so a repo can be
  // dragged from one lane onto another to reassign it (issue #208). Rebuilt from
  // `repos` imperatively (never reactively) so it never fights svelte-dnd-action
  // mid-drag; a settled drag reconciles back to this from the server's truth.
  let reposByRailway = $state<Record<string, Repository[]>>({})
  const FLIP_MS = 150

  // The create-lane form.
  let newRailwayName = $state('')
  let newRailwayDescription = $state('')

  // Per-railway in-place edits of name + description, keyed by railway id, so an
  // edit is not lost until saved. Seeded whenever `railways` is (re)loaded, never
  // during render, so reading it from the template is a pure lookup.
  type RailwayEdit = { name: string; description: string }
  let railwayEdits = $state<Record<string, RailwayEdit>>({})

  // The id of the railway whose start/stop/delete action is in flight, so its
  // buttons can disable without freezing the whole panel.
  let railwayBusyId = $state<string | null>(null)
  let railwayToDelete = $state<Railway | null>(null)
  let railwayIdleSavedAt = $state<string | null>(null)

  const RAILWAY_STATE_LABELS = {
    stopped: 'Stopped',
    starting: 'Starting',
    running: 'Running',
    stopping: 'Stopping'
  } as const satisfies Record<RailwayState, string>

  const RAILWAY_STATE_BADGE = {
    stopped: 'text-muted-foreground',
    starting: 'border-primary/40 text-primary',
    running: 'border-success/40 text-success',
    stopping: 'border-warning/40 text-warning'
  } as const satisfies Record<RailwayState, string>

  // Replaces the railways list and re-seeds the editable name/description rows. We
  // only fill in a missing entry so an unsaved edit on an existing lane survives a
  // background refresh, while a freshly loaded lane still gets its editable copy.
  function setRailways(next: Railway[]) {
    railways = next
    for (const railway of next) {
      railwayEdits[railway.id] ??= { name: railway.name, description: railway.description }
    }
  }

  async function refreshRailways() {
    setRailways(await listRailways())
  }

  // Persists the lane idle-stop timeout (minutes a non-main railway may sit idle
  // before its container is stopped). 0 or less disables idle-stopping.
  async function saveRailwayIdleTimeout() {
    if (!settings) {
      return
    }
    settings = await updateSettings({
      railway_idle_timeout_minutes: settings.railway_idle_timeout_minutes
    })
    railwayIdleSavedAt = new Date().toLocaleTimeString()
  }

  async function addRailway() {
    const name = newRailwayName.trim()
    if (!name) {
      return
    }
    try {
      await createRailway({ name, description: newRailwayDescription.trim() })
      newRailwayName = ''
      newRailwayDescription = ''
      await refreshRailways()
      groupRepos()
      toast.success(`Created the ${name} railway.`)
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not create the railway.'))
    }
  }

  async function saveRailway(railway: Railway) {
    const edit = railwayEdits[railway.id]
    if (!edit || !edit.name.trim()) {
      return
    }
    try {
      const updated = await updateRailway(railway.id, {
        name: edit.name.trim(),
        description: edit.description.trim()
      })
      setRailways(railways.map((existing) => (existing.id === updated.id ? updated : existing)))
      toast.success('Saved the railway.')
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not save the railway.'))
    }
  }

  async function toggleRailwayPause(railway: Railway) {
    try {
      const updated = await setRailwayPaused(railway.id, !railway.paused)
      setRailways(railways.map((existing) => (existing.id === updated.id ? updated : existing)))
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not update the railway pause.'))
    }
  }

  async function runStartRailway(railway: Railway) {
    railwayBusyId = railway.id
    try {
      const result = await startRailway(railway.id)
      await refreshRailways()
      toast.success(result.message ?? 'Started the railway.')
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not start the railway.'))
    } finally {
      railwayBusyId = null
    }
  }

  async function runStopRailway(railway: Railway) {
    railwayBusyId = railway.id
    try {
      const result = await stopRailway(railway.id)
      await refreshRailways()
      toast.success(result.message ?? 'Stopped the railway.')
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not stop the railway.'))
    } finally {
      railwayBusyId = null
    }
  }

  async function confirmDeleteRailway() {
    const railway = railwayToDelete
    if (!railway) {
      return
    }
    railwayBusyId = railway.id
    try {
      await deleteRailway(railway.id)
      railwayToDelete = null
      await refreshRailways()
      // Repos fall back to main, so refresh their lane assignments too.
      repos = await listRepos()
      groupRepos()
      toast.success(`Deleted the ${railway.name} railway.`)
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not delete the railway.'))
    } finally {
      railwayBusyId = null
    }
  }

  function railwayName(railwayId: string): string {
    return railways.find((railway) => railway.id === railwayId)?.name ?? 'the railway'
  }

  // Rebuilds the per-railway repo buckets from `repos`. Every railway gets a
  // bucket (so empty lanes are still drop targets); a repo whose lane no longer
  // exists falls back to main. Called after loads and after a settled drag, never
  // during one.
  function groupRepos() {
    const grouped: Record<string, Repository[]> = {}
    for (const railway of railways) {
      grouped[railway.id] = []
    }
    const mainId = railways.find((railway) => railway.is_main)?.id
    for (const repo of repos) {
      const laneId = grouped[repo.railway_id] ? repo.railway_id : mainId
      if (laneId && grouped[laneId]) {
        grouped[laneId].push(repo)
      }
    }
    reposByRailway = grouped
  }

  // svelte-dnd-action mutates the dragged-over bucket live; mirror it into state.
  function handleRepoConsider(railwayId: string, event: CustomEvent<DndEvent<Repository>>) {
    reposByRailway[railwayId] = event.detail.items
  }

  // A repo dropped onto a lane: reassign it (and its tasks) to that railway. The
  // backend blocks the move while a live turn is working the repo on its current
  // lane and returns the reason, which we surface in a toast. Either way we
  // reconcile the buckets from the server's truth, so a rejected drop snaps back.
  async function handleRepoFinalize(railwayId: string, event: CustomEvent<DndEvent<Repository>>) {
    reposByRailway[railwayId] = event.detail.items
    const movedId = event.detail.info.id
    const moved = event.detail.items.find((repo) => repo.id === movedId)
    // Finalize fires on both the source and destination zones; only the
    // destination holds the dragged repo. A same-lane drop is just a reorder
    // (repos have no order within a lane), so there is nothing to persist.
    if (!moved || moved.railway_id === railwayId) {
      groupRepos()
      return
    }
    try {
      const updated = await assignRepoToRailway(railwayId, moved.id)
      repos = repos.map((existing) => (existing.id === updated.id ? updated : existing))
      toast.success(`Moved ${moved.full_name} to ${railwayName(railwayId)}.`)
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not move the repo to that railway.'))
    } finally {
      groupRepos()
    }
  }

  // Toggle the global master pause (the same switch the board exposes). Shown here
  // so the relationship between it and the per-railway pause is clear.
  async function toggleMasterPause() {
    if (!settings) {
      return
    }
    try {
      settings = await setPaused(!settings.agent_paused)
    } catch (error) {
      toast.error(await extractApiError(error, 'Could not update the master pause.'))
    }
  }

  async function load() {
    settings = await getSettings()
    repos = await listRepos()
    setRailways(await listRailways())
    groupRepos()
  }

  onMount(load)
</script>

<div class="mx-auto max-w-4xl space-y-6 p-6">
  <header>
    <h1 class="text-2xl font-bold tracking-tight">Railways</h1>
    <p class="mt-1 max-w-2xl text-sm text-muted-foreground">
      Railways are parallel agent lanes. Each has its own workspace container, agent loop, Claude
      session, and set of repositories. A repository belongs to exactly one railway, so a task's
      lane always follows its repo. The <strong>main</strong> railway holds everything by default
      and cannot be deleted or stopped. Lane containers start lazily on first work and idle-stop on
      their own; you can also start or stop them by hand here.
    </p>
  </header>

  {#if settings}
  <Card.Root>
    <Card.Content class="space-y-6 pt-6">
      <!-- Global master pause, shown here for context: it gates every lane. The
           per-railway pause below gates one lane; either being on stops a lane. -->
      <div class="flex items-start gap-2 rounded-md border border-border p-3">
        <Switch id="master-pause" checked={settings.agent_paused} onCheckedChange={toggleMasterPause} />
        <div class="grid gap-1">
          <Label for="master-pause">Master pause (all railways)</Label>
          <span class="text-xs text-muted-foreground">
            The global pause from the board. While on, no railway pulls new work. Each railway also
            has its own pause below; either one stops that lane.
          </span>
        </div>
      </div>

      <!-- Idle-stop timeout: how long a non-main lane may sit with no work
           before its container is stopped to free memory. Applies to every
           non-main lane; main is the always-on compose workspace. -->
      <div class="space-y-3 rounded-md border border-border p-3">
        <div class="space-y-1.5">
          <Label for="railway-idle-timeout">Idle-stop timeout</Label>
          <div class="flex items-center gap-2">
            <Input
              id="railway-idle-timeout"
              type="number"
              min="0"
              class="w-24"
              bind:value={settings.railway_idle_timeout_minutes}
            />
            <span class="text-sm text-muted-foreground">minutes of no work</span>
          </div>
          <p class="text-xs leading-relaxed text-muted-foreground">
            After this many minutes with no work, a non-main lane's container is stopped (its clones
            and Claude session are kept, so a restart is fast). Set to <strong>0</strong> to never
            idle-stop and leave lanes running until stopped by hand. The <strong>main</strong> lane
            is never idle-stopped.
          </p>
        </div>
        <div class="flex items-center gap-3">
          <Button size="sm" onclick={saveRailwayIdleTimeout}>Save idle timeout</Button>
          {#if railwayIdleSavedAt}
            <span class="text-sm text-muted-foreground">Saved at {railwayIdleSavedAt}</span>
          {/if}
        </div>
      </div>

      <!-- Create a new lane. -->
      <div class="space-y-3 rounded-md border border-border p-3">
        <h3 class="text-sm font-semibold">Create a railway</h3>
        <div class="space-y-1.5">
          <Label for="new-railway-name">Name</Label>
          <Input id="new-railway-name" placeholder="e.g. Frontend" bind:value={newRailwayName} />
        </div>
        <div class="space-y-1.5">
          <Label for="new-railway-description">Description</Label>
          <Input
            id="new-railway-description"
            placeholder="Optional. What this lane is for."
            bind:value={newRailwayDescription}
          />
        </div>
        <Button size="sm" disabled={!newRailwayName.trim()} onclick={addRailway}>Create railway</Button>
      </div>

      <!-- One editable block per railway. `edit` is a pure read of the map seeded
           in setRailways; the `?? { ... }` fallback guards the brief window after a
           create before the refresh lands so a bind never sees an undefined row. -->
      {#each railways as railway (railway.id)}
        {@const edit = railwayEdits[railway.id] ?? { name: railway.name, description: railway.description }}
        <div class="space-y-3 rounded-lg border border-border p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex flex-wrap items-center gap-2">
              <span class="font-medium">{railway.name}</span>
              {#if railway.is_main}
                <Badge variant="outline" class="border-primary/40 text-primary">main</Badge>
              {/if}
              <Badge variant="outline" class={RAILWAY_STATE_BADGE[railway.lifecycle_state]}>
                {RAILWAY_STATE_LABELS[railway.lifecycle_state]}
              </Badge>
              {#if railway.paused}
                <Badge variant="outline" class="border-warning/40 text-warning">paused</Badge>
              {/if}
            </div>
            <div class="flex items-center gap-2">
              <Switch
                id={`railway-pause-${railway.id}`}
                checked={railway.paused}
                onCheckedChange={() => toggleRailwayPause(railway)}
              />
              <Label for={`railway-pause-${railway.id}`} class="text-xs">Pause this lane</Label>
            </div>
          </div>

          <!-- Rename + describe. -->
          <div class="grid gap-2 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label for={`railway-name-${railway.id}`}>Name</Label>
              <Input id={`railway-name-${railway.id}`} bind:value={edit.name} />
            </div>
            <div class="space-y-1.5">
              <Label for={`railway-description-${railway.id}`}>Description</Label>
              <Input id={`railway-description-${railway.id}`} bind:value={edit.description} />
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={!edit.name.trim()} onclick={() => saveRailway(railway)}>
              Save
            </Button>
            <!-- main is always running and cannot be started/stopped or deleted. -->
            {#if !railway.is_main}
              {#if railway.lifecycle_state === 'running' || railway.lifecycle_state === 'starting'}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={railwayBusyId === railway.id}
                  onclick={() => runStopRailway(railway)}
                >
                  Stop
                </Button>
              {:else}
                <Button
                  variant="outline"
                  size="sm"
                  disabled={railwayBusyId === railway.id}
                  onclick={() => runStartRailway(railway)}
                >
                  Start
                </Button>
              {/if}
              <Button
                variant="ghost"
                size="sm"
                class="text-destructive hover:text-destructive"
                disabled={railwayBusyId === railway.id}
                onclick={() => (railwayToDelete = railway)}
              >
                Delete
              </Button>
            {/if}
          </div>
        </div>
      {/each}

      <!-- Repository to railway assignment. One drop zone per lane holds that
           lane's repos; drag a repo card to another lane to move it (and its
           tasks). The move calls the assign API and respects its block-while-
           working guard, surfacing the reason on rejection (issue #208). -->
      <div class="space-y-3 border-t border-border pt-5">
        <div>
          <h3 class="text-sm font-semibold">Repository assignments</h3>
          <p class="mt-1 text-sm text-muted-foreground">
            Each repository works on exactly one railway. Drag a repo from one lane to another to
            move it (and all of its tasks). A move is blocked while the agent is mid-turn on that
            repo's current lane.
          </p>
        </div>
        {#if repos.length === 0}
          <p class="text-sm text-muted-foreground">No repositories configured yet.</p>
        {:else}
          <div class="grid gap-3 sm:grid-cols-2">
            {#each railways as railway (railway.id)}
              {@const laneRepos = reposByRailway[railway.id] ?? []}
              <div class="rounded-lg border border-border">
                <div class="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
                  <span class="truncate text-sm font-medium">
                    {railway.name}
                    {#if railway.is_main}
                      <span class="text-xs font-normal text-muted-foreground">(default)</span>
                    {/if}
                  </span>
                  <span class="text-xs tabular-nums text-muted-foreground">{laneRepos.length}</span>
                </div>
                <div class="relative">
                  <div
                    class="flex min-h-[3.5rem] flex-col gap-2 p-2"
                    use:dndzone={{
                      items: laneRepos,
                      type: 'railway-repo',
                      flipDurationMs: FLIP_MS,
                      dropTargetStyle: {},
                      dropTargetClasses: ['ring-2', 'ring-primary/50', 'rounded-lg']
                    }}
                    onconsider={(event) => handleRepoConsider(railway.id, event)}
                    onfinalize={(event) => handleRepoFinalize(railway.id, event)}
                  >
                    {#each laneRepos as repo (repo.id)}
                      <div
                        class="cursor-grab truncate rounded-md border border-border bg-card px-2.5 py-1.5 text-sm shadow-sm active:cursor-grabbing"
                        title={repo.full_name}
                      >
                        {repo.full_name}
                      </div>
                    {/each}
                  </div>
                  {#if laneRepos.length === 0}
                    <p class="pointer-events-none absolute inset-0 grid place-items-center text-xs text-muted-foreground">
                      Drop a repo here
                    </p>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </Card.Content>
  </Card.Root>

  <!-- Delete confirmation: main is undeletable, so this only ever targets a
       non-main lane; the API reassigns its repos to main and tears it down. -->
  <AlertDialog.Root open={railwayToDelete !== null} onOpenChange={(open) => !open && (railwayToDelete = null)}>
    <AlertDialog.Content>
      <AlertDialog.Header>
        <AlertDialog.Title>Delete the {railwayToDelete?.name} railway?</AlertDialog.Title>
        <AlertDialog.Description>
          Its repositories and their tasks move back to <strong>main</strong>, then its container is
          torn down and its Claude session is cleared. This is blocked while a live turn is running
          on the lane. This cannot be undone.
        </AlertDialog.Description>
      </AlertDialog.Header>
      <AlertDialog.Footer>
        <AlertDialog.Cancel>Cancel</AlertDialog.Cancel>
        <Button
          variant="destructive"
          disabled={railwayBusyId === railwayToDelete?.id}
          onclick={confirmDeleteRailway}
        >
          Delete railway
        </Button>
      </AlertDialog.Footer>
    </AlertDialog.Content>
  </AlertDialog.Root>
  {/if}
</div>
