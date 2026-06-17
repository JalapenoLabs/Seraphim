# Visual regression baselines (opt-in, per repo)

This is the long-game complement to the per-change visual self-review (see
`visual-checks.md`). Once a screen looks right, lock it in with a committed
**baseline screenshot** so a later, unrelated change can't silently break it. The
per-change loop catches "is this change right?"; baselines catch "did this change
break something that was already right?".

It is **opt-in per repo** and runs in the repo's own test suite, NOT through the
Playwright MCP: pixel comparison needs the standalone `@playwright/test` runner
(`toHaveScreenshot()`), which the MCP does not provide. Only set this up, or run
it, in a repo that has opted in (its `CLAUDE.md` says so, or the task is to add
it). Do not add baselines to a repo that has not asked for them.

**Baselines live in the repo under test, committed alongside its tests, never in
Seraphim.**

## Where baselines live

`toHaveScreenshot()` writes PNGs next to the spec, under
`<spec>.ts-snapshots/<name>-<project>-<platform>.png` by default. Commit that
directory. Because rendering differs across OS and font stacks, a baseline is only
valid on the platform that generated it, so the golden rule is:

> Generate and compare baselines in the SAME environment CI uses.

Pin the Playwright version (it pins the browser build) and run both the
baseline-generation and the CI diff inside the same container, e.g. the official
`mcr.microsoft.com/playwright:vX.Y.Z-jammy` image (or whatever image CI runs).
Generating a baseline on your host and diffing it in CI is the most common source
of "works locally, fails in CI" noise. The `{platform}` token in the path keeps
per-OS baselines separate when a repo genuinely needs more than one.

## Setup (once, in the opted-in repo)

1. Add the runner: `yarn add -D @playwright/test` (then `yarn playwright install
   --with-deps chromium`, already baked into this workspace image).
2. Add a `playwright.config.ts` tuned against flakiness:

```ts
import { defineConfig, devices } from '@playwright/test'

export default defineConfig({
  testDir: './tests/visual',
  // Fail CI if a baseline is missing rather than silently creating one: a new
  // baseline must be a deliberate, reviewed commit, never a side effect of CI.
  forbidOnly: !!process.env.CI,
  use: {
    baseURL: 'http://localhost:5173',
    viewport: { width: 1280, height: 800 }, // fixed, never the window size
    deviceScaleFactor: 1,
    reducedMotion: 'reduce',
    colorScheme: 'light',
    timezoneId: 'UTC', // freeze locale-dependent rendering
    locale: 'en-US',
  },
  expect: {
    toHaveScreenshot: {
      // Disable animations and hide the blinking caret so timing can't flip a pixel.
      animations: 'disabled',
      caret: 'hide',
      scale: 'css',
      // A tiny tolerance absorbs sub-pixel anti-aliasing without hiding real diffs.
      maxDiffPixelRatio: 0.01,
    },
  },
})
```

## The spec

```ts
import { test, expect } from '@playwright/test'

test('dashboard looks right', async ({ page }) => {
  await page.goto('/dashboard')
  // Wait for the real "settled" signal, not a fixed sleep: fonts loaded and the
  // content present. A hard timeout is itself a flake source.
  await page.locator('[data-testid="dashboard-ready"]').waitFor()
  await page.evaluate(() => document.fonts.ready)

  await expect(page).toHaveScreenshot('dashboard.png', {
    // Mask regions whose content legitimately changes run to run, so they never
    // trip the diff: clocks, relative times, avatars, random ids, live counts.
    mask: [page.locator('.timestamp'), page.locator('[data-dynamic]')],
  })
})
```

## Controlling flakiness (do all of these)

A flaky baseline is worse than none: it trains everyone to ignore the diff. Pin
down every source of nondeterminism:

- **Fixed viewport + `deviceScaleFactor: 1`** so the canvas size never varies.
- **Animations disabled, caret hidden** (set above) so timing can't change a pixel.
- **Deterministic data:** point the dev server at seeded/fixture data or mock the
  network (`page.route(...)`), and freeze the clock (`timezoneId: 'UTC'`, and stub
  `Date`/`Math.random` if the UI shows them). Never baseline live/production data.
- **Mask dynamic regions** (above) instead of widening the global threshold.
- **Wait on a real readiness signal** (a test id, `document.fonts.ready`, a
  network-idle for the relevant requests), never `waitForTimeout`.
- **Same environment for generate and compare** (see "Where baselines live").
- Keep the diff tolerance small (`maxDiffPixelRatio` ~0.01); if a screen needs
  more, mask the noisy part rather than blinding the whole shot.

## Workflow

1. **Establish** a baseline only when the screen is confirmed good (you have just
   reviewed it via the per-change loop): `yarn playwright test --update-snapshots`,
   then **commit** the generated `*-snapshots/` PNGs. Review the committed images
   like any other artifact, a wrong baseline locks in a bug.
2. **Diff on later changes:** `yarn playwright test`. CI runs the same. A pass
   means the rendered pixels match the committed baseline.
3. **React to a regression:** an unexpected diff is a **failure to investigate**,
   not a baseline to bump. Open the report (`playwright-report/`, with
   expected/actual/diff images) and decide:
   - The change was NOT meant to alter this screen → it is a real regression; fix
     the code until the diff is clean.
   - The change WAS meant to alter this screen → the new rendering is correct;
     re-run with `--update-snapshots`, eyeball the new image, and commit it with a
     message saying why it changed. Updating the baseline is a deliberate act.

Never reflexively `--update-snapshots` to make a red check green: that is how a
fixed bug gets re-introduced. That re-introduction is exactly what baselines exist
to stop.

## Opting a repo in

Add the dependency, the `playwright.config.ts`, the specs under `tests/visual/`,
the committed baselines, and a `"test:visual": "playwright test"` script. Wire it
into CI in the same container that generated the baselines. Record in the repo's
own `CLAUDE.md` that visual regression is enabled, where the specs and baselines
live, and the command to update a baseline, so the next run knows it is opt-in and
how to use it.

## Heavier alternative: Storybook + Chromatic

For a component-library-driven codebase, Storybook + Chromatic does per-component
visual regression as a hosted service (automatic baselines, a review UI, parallel
cross-browser snapshots) instead of committed PNGs. It is more setup and a paid
dependency, so treat it as an option for repos already invested in Storybook, not
the default. The committed-baseline recipe above is the lightweight default.

## Worked example: catching a regression

A baseline exists for `dashboard.png` (committed). A later task tweaks a shared
button's padding, not meant to touch the dashboard. CI runs `playwright test`:

```
  1) dashboard.spec.ts:3:1 › dashboard looks right ─────────────────────────────

    Error: Screenshot comparison failed:

      1248 pixels (ratio 0.02 of all image pixels) are different.

    Expected: dashboard-linux.png
    Received: dashboard-actual.png
    Diff:     dashboard-diff.png
```

The diff image shows the dashboard's action button shifted, the shared-padding
change leaked here. Because it was unintended, this is a regression: revert/correct
the padding until `playwright test` is green again. The baseline is left untouched,
and a bug that would have shipped silently was caught instead.
