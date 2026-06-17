# Compose from the design system (constrain the spacing decision)

The deepest reason spacing and alignment come out wrong is that they are a free
decision on every single component: each new bit of UI re-invents its margins from
scratch, so there are endless ways to be slightly off. The fix is to cut the
degrees of freedom. Before you reach for a `margin`, reach for the vocabulary the
repo already has.

This is build-time guidance (how to write the UI). The per-change verification of
the result lives in `visual-checks.md` (computed-style checks).

## Prefer primitives and tokens over ad-hoc spacing

- **Layout primitives.** If the repo has them, express spacing as a choice from a
  fixed set: a `<Stack gap="md">`, `<Inline>`, `<Grid>`, `<Box p="4">`, etc. A
  primitive with a `gap`/`p` prop drawn from the scale is far harder to get wrong
  than a hand-written `margin-top: 13px`.
- **Spacing-scale tokens.** Use the named steps the repo defines (`xs`/`sm`/`md`,
  `1`/`2`/`3`/`4`, Tailwind's spacing scale, CSS `--space-*` vars) instead of
  arbitrary pixel values. A value that is not on the scale is almost always a bug.
- **Shared components.** Reuse the repo's own components or its component library
  (shadcn/ui, Radix, MUI, the repo's `components/ui`) rather than re-implementing a
  button / card / field with bespoke padding. Change spacing through a component's
  props or variants, not by wrapping it in a one-off `div` with a margin.

## Match the neighbors

Before writing any layout, look at the sibling components around your change: which
primitives, token names, and spacing steps do they use? Match them. Grep for the
same element type elsewhere in the codebase and copy its conventions. The goal is
to look like it was always there, not to introduce a new spacing dialect.

## Discover the repo's vocabulary

- **Tokens / scale:** `tailwind.config.*` (`theme.spacing`), a `theme.ts` /
  `tokens.*`, or CSS custom properties (`--space-*`, `--radius-*`).
- **Primitives:** a `components/ui/` or `lib/primitives/` directory, a design-system
  package, or a UI library already in `package.json`.
- If the repo records its design system in its own `CLAUDE.md`, follow it. If you
  discover the vocabulary and it is not recorded, add a short note there (the tokens
  file, the primitives dir, the spacing scale) so the next run starts informed.

## Anti-patterns

- Magic values: `margin: 13px`, `style={{ marginTop: 17 }}`, a `gap: 11px` that is
  not on any scale.
- A bespoke `display: flex; gap: ...` where a `<Stack>` / `<Inline>` primitive
  already exists.
- Re-implementing or wrapping a shared component just to nudge its spacing, instead
  of using the prop/variant it already exposes.

## Optional: implement against a Figma spec

Only when a **Figma MCP is configured** for the repo (none is wired by default, so
treat this as opt-in and skip it otherwise): use the Figma MCP to pull the actual
spec, the real spacing, sizes, and token values, and implement against those exact
numbers instead of eyeballing them. Then validate the implementation against that
spec with the computed-style checks (`/usr/local/share/seraphim/visual-checks.md`)
at mobile (375px) and desktop (1280px): the Figma numbers become the expected
values your centering/spacing assertions check, so "matches the design" becomes a
pass/fail measurement rather than a judgment call. With no Figma MCP and no spec,
fall back to matching the neighbors and the repo's token scale.

## Where the primitives live

The concrete tokens, scale, and primitives live in each repo under test, not in
Seraphim. This is guidance for picking from whatever vocabulary a repo already has.
If a repo genuinely has none, still prefer a small consistent scale (a handful of
spacing steps) over scattered magic numbers, and record what you chose in that
repo's `CLAUDE.md` so it becomes the convention the next change matches.
