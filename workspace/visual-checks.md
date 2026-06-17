# Computed-style checks (the agent's eyes)

This is the workhorse for visual self-review. When you change something visible,
verify the layout with these deterministic measurements **first**, and reach for
a screenshot only to confirm the final result. A screenshot is ambiguous and
token-expensive; a computed-style check returns a fact ("left gap 8px, right gap
96px"), so "is it centered?" becomes a pass/fail assertion you can trust.

Run the checks through the Playwright MCP `browser_evaluate` tool against the
page you changed. Repeat them at **both** widths a layout has to survive:

1. `browser_resize` to **375 x 812** (mobile), navigate/render, run the checks.
2. `browser_resize` to **1280 x 800** (desktop), run the checks again.

How to find the dev server URL and routes for the repo you are working on is in
that repo's `CLAUDE.md`. If the repo has no runnable UI, skip this review.

## The library

Paste this whole function into `browser_evaluate`, edit the `spec` at the bottom
for the page under review, and read the returned report. `pass` is the overall
verdict; each entry carries the measured numbers behind its own `pass`.

```js
() => {
  // Every check is a deterministic measurement of the live layout, so a failure
  // is a fact you can act on, not a guess. Tolerances are in CSS pixels and
  // default low (~1-2px) to absorb sub-pixel rounding without hiding real bugs.

  const round = (n) => Math.round(n * 100) / 100;

  // A short, findable handle for an element, e.g. "button#submit.primary".
  const describe = (el) => {
    if (!el) return "(none)";
    const id = el.id ? `#${el.id}` : "";
    const cls =
      typeof el.className === "string" && el.className.trim()
        ? "." + el.className.trim().split(/\s+/).join(".")
        : "";
    return `${el.tagName.toLowerCase()}${id}${cls}`;
  };

  const requireEl = (selector) => {
    const el = document.querySelector(selector);
    if (!el) throw new Error(`selector matched nothing: ${selector}`);
    return el;
  };

  // The content box (border box minus borders and padding) in viewport
  // coordinates. Centering and spacing measure against the container's content
  // box so its padding is never mistaken for an offset.
  const contentBox = (el) => {
    const rect = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      left: rect.left + parseFloat(cs.borderLeftWidth) + parseFloat(cs.paddingLeft),
      right: rect.right - parseFloat(cs.borderRightWidth) - parseFloat(cs.paddingRight),
      top: rect.top + parseFloat(cs.borderTopWidth) + parseFloat(cs.paddingTop),
      bottom: rect.bottom - parseFloat(cs.borderBottomWidth) - parseFloat(cs.paddingBottom),
    };
  };

  // Centered inside its container? Compares the gap on each side of the element
  // within the container's content box; equal gaps (within `tolerance`) means
  // centered. `axis` is "horizontal" (default), "vertical", or "both".
  // `container` defaults to the element's parent.
  const centering = ({ selector, container, axis = "horizontal", tolerance = 1.5 }) => {
    const el = requireEl(selector);
    const box = contentBox(container ? requireEl(container) : el.parentElement);
    const rect = el.getBoundingClientRect();
    const axes = axis === "both" ? ["horizontal", "vertical"] : [axis];
    const measured = {};
    let pass = true;
    for (const a of axes) {
      const before = a === "horizontal" ? rect.left - box.left : rect.top - box.top;
      const after = a === "horizontal" ? box.right - rect.right : box.bottom - rect.bottom;
      const offset = round(before - after);
      measured[a] = { before: round(before), after: round(after), offset };
      if (Math.abs(offset) > tolerance) pass = false;
    }
    return {
      name: `centering ${selector}`,
      pass,
      detail: pass ? "gaps are equal within tolerance" : "unequal gaps: element is off-center",
      measured: { axis, tolerance, ...measured },
    };
  };

  // The elements (in document order) sit at the expected gap with no overlap?
  // Pass `expected` for an exact gap (within `tolerance`), or `min` for a floor.
  // `axis` is "horizontal" (default) or "vertical".
  const spacing = ({ selectors, axis = "horizontal", expected, min, tolerance = 1.5 }) => {
    const els = selectors.map(requireEl);
    const gaps = [];
    let pass = true;
    for (let i = 0; i < els.length - 1; i++) {
      const a = els[i].getBoundingClientRect();
      const b = els[i + 1].getBoundingClientRect();
      const gap = round(axis === "horizontal" ? b.left - a.right : b.top - a.bottom);
      gaps.push({ between: [describe(els[i]), describe(els[i + 1])], gap });
      if (gap < 0) pass = false; // negative gap means the two controls overlap
      if (expected != null && Math.abs(gap - expected) > tolerance) pass = false;
      if (min != null && gap < min - tolerance) pass = false;
    }
    return {
      name: `spacing ${selectors.join(" / ")}`,
      pass,
      detail: pass ? "gaps within expectation, none overlapping" : "gap out of range or elements overlap",
      measured: { axis, expected, min, tolerance, gaps },
    };
  };

  // Anything causing an unintended horizontal scrollbar, or spilling past the
  // viewport edges? Reports the worst offenders so the culprit is easy to find.
  const overflow = ({ tolerance = 1 } = {}) => {
    const doc = document.documentElement;
    const overshoot = round(doc.scrollWidth - doc.clientWidth);
    const offenders = [];
    for (const el of document.body.querySelectorAll("*")) {
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      const spill = round(Math.max(rect.right - window.innerWidth, -rect.left));
      if (spill > tolerance) offenders.push({ element: describe(el), spillPx: spill });
    }
    offenders.sort((a, b) => b.spillPx - a.spillPx);
    const pass = overshoot <= tolerance;
    return {
      name: "horizontal overflow",
      pass,
      detail: pass ? "no horizontal overflow" : `document scrolls ${overshoot}px past the viewport`,
      measured: {
        scrollWidth: doc.scrollWidth,
        clientWidth: doc.clientWidth,
        overshoot,
        offenders: offenders.slice(0, 5),
      },
    };
  };

  // Does `top` actually render above `under` where they overlap? Samples the
  // center of the overlap with elementFromPoint; the hit must be `top` or a
  // descendant. Catches overlays and menus that are visually behind content.
  const stacking = ({ top, under }) => {
    const topEl = requireEl(top);
    const underEl = requireEl(under);
    const a = topEl.getBoundingClientRect();
    const b = underEl.getBoundingClientRect();
    const overlaps = a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top;
    const x = round((Math.max(a.left, b.left) + Math.min(a.right, b.right)) / 2);
    const y = round((Math.max(a.top, b.top) + Math.min(a.bottom, b.bottom)) / 2);
    const hit = overlaps ? document.elementFromPoint(x, y) : null;
    const pass = overlaps ? !!(hit && (hit === topEl || topEl.contains(hit))) : true;
    return {
      name: `stacking ${top} over ${under}`,
      pass,
      detail: !overlaps
        ? "elements do not overlap; nothing to stack"
        : pass
          ? `${top} is on top at the overlap`
          : `${describe(hit)} covers ${top} at the overlap`,
      measured: { overlaps, point: { x, y }, hit: describe(hit) },
    };
  };

  // Runs every check named in the spec and rolls them into one report. A bad
  // selector fails only its own check, so the rest of the report still returns.
  const runVisualChecks = (spec) => {
    const safe = (fn, arg) => {
      try {
        return fn(arg);
      } catch (e) {
        return { name: fn.name, pass: false, detail: `check errored: ${e.message}`, measured: { arg } };
      }
    };
    const results = [];
    for (const c of spec.center || []) results.push(safe(centering, c));
    for (const s of spec.spacing || []) results.push(safe(spacing, s));
    if (spec.overflow) results.push(safe(overflow, spec.overflow === true ? {} : spec.overflow));
    for (const s of spec.stacking || []) results.push(safe(stacking, s));
    return {
      viewport: { width: window.innerWidth, height: window.innerHeight },
      pass: results.every((r) => r.pass),
      results,
    };
  };

  // ----- edit this spec for the page under review -----
  return runVisualChecks({
    center: [{ selector: "#submit" }],
    spacing: [{ selectors: ["#cancel", "#submit"], min: 8 }],
    overflow: true,
    stacking: [{ top: "#menu", under: "main" }],
  });
}
```

### The spec

- `center`: `[{ selector, container?, axis?, tolerance? }]` - each element that
  should be centered. `axis` is `"horizontal"` (default), `"vertical"`, or
  `"both"`; `container` defaults to the element's parent.
- `spacing`: `[{ selectors, axis?, expected?, min?, tolerance? }]` - `selectors`
  is an ordered list of siblings. Give `expected` for an exact gap or `min` for
  a floor; either way a negative gap (overlap) fails.
- `overflow`: `true` (or `{ tolerance }`) - flags an unintended horizontal
  scrollbar and lists what spills past the viewport.
- `stacking`: `[{ top, under }]` - asserts `top` paints above `under` where they
  overlap (overlays, dropdown menus, modals).

## Worked example: catching an off-center, wrong-gap layout

Suppose a card is supposed to be centered in its container and its two footer
buttons are supposed to sit 12px apart, but a stray `margin-left` and a missing
`gap` shipped instead:

```html
<div id="container" style="width: 600px; padding: 20px;">
  <!-- BUG: margin-left shoves the card off-center -->
  <div id="card" style="width: 300px; margin-left: 80px;">
    <div id="footer" style="display: flex;">
      <button id="cancel">Cancel</button>
      <!-- BUG: no gap, so the buttons touch -->
      <button id="save">Save</button>
    </div>
  </div>
</div>
```

Run the checks:

```js
runVisualChecks({
  center: [{ selector: "#card", container: "#container" }],
  spacing: [{ selectors: ["#cancel", "#save"], expected: 12 }],
});
```

The report fails, and says exactly why:

```json
{
  "viewport": { "width": 1280, "height": 800 },
  "pass": false,
  "results": [
    {
      "name": "centering #card",
      "pass": false,
      "detail": "unequal gaps: element is off-center",
      "measured": { "axis": "horizontal", "tolerance": 1.5,
        "horizontal": { "before": 80, "after": 220, "offset": -140 } }
    },
    {
      "name": "spacing #cancel / #save",
      "pass": false,
      "detail": "gap out of range or elements overlap",
      "measured": { "axis": "horizontal", "expected": 12,
        "gaps": [{ "between": ["button#cancel", "button#save"], "gap": 0 }] }
    }
  ]
}
```

The numbers point straight at the fix: drop `margin-left: 80px` (use
`margin: 0 auto` so `before` and `after` match) and add `gap: 12px` to the flex
footer. Re-running returns `"pass": true` with `offset` near `0` and `gap` near
`12`. Only then is a confirming screenshot worth the tokens.
