# fnscope workspace — design-validation prototype

A static mock of the proposed workspace UI: isolated project scopes, an entry-point /
module explorer, an execution-story canvas, and an invoke sandbox. Nothing here is wired
into the real app. `ui/src`, the server, and the locked Rust API are untouched.

## Open it

The prototype is served at **http://localhost:4320/**.

To start it yourself:

```bash
cd wireframe/fnscope-workspace
python3 -m http.server 4320
# Windows PowerShell: python -m http.server 4320
```

Then open `http://localhost:4320/`.

Tailwind, Lucide and the fonts load from CDNs, so the layout needs a network connection.
The data itself is local.

## What is real

- **Functions, counts, files, parameters, callers, errors and step counts** come from a real
  fnscope self-scan (`.fnscope/fn-graph.json`, 26 Nov 2026).
- **Three scopes** are slices of that one scan:
  - `fnscope-crates` — 981 functions, 51 files, Rust only. The clean core implementation.
  - `fnscope` — 1,068 functions, 76 files, 14 languages. The full tree, explicitly labelled
    as including `tests/fixtures`.
  - `sample-nlp-app` — 9 functions, 8 files. The fixture app treated as its own scope.
- **Stories for `invoke`, `scan_with_registry` and `router`** are curated from their real
  source. Every other function shows trimmed manifest steps.
- **Source code** is embedded only for curated functions.

Everything else is simulated: no source is executed, no manifest is rescanned, and “Open
source” is a stub.

## Palette

The first pass had the default AI-dashboard look: near-black slate, indigo/violet gradients,
rainbow language badges, glowing dots. That is gone.

- **Paper (default)** — warm off-white paper, near-black ink, one vermilion accent. Reads like
  a technical document rather than a SaaS dashboard.
- **Graphite** — warm charcoal, bone text, copper accent. Same structure, quieter at night.

Toggle them with the moon/sun button in the top bar. The choice is remembered across reloads.

Both palettes use IBM Plex Sans / Mono / Serif instead of Inter / JetBrains Mono. Gradients,
glows and heavy shadows are removed; corners are 4–8px; language badges are neutral; the accent
is reserved for primary actions, the current selection and the current function.

## Decisions I made without asking

1. **Standalone prototype, not a live UI change.** You asked to validate first; editing
   `ui/src` now would mix design risk with implementation risk.
2. **Default scope is `fnscope-crates`, not the full repo.** The full tree still contains
   fixture code, which is the pollution you flagged. The clean core is the honest default.
3. **The full tree stays as its own scope.** It is useful for comparison, and it now carries
   an explicit “tests/fixtures included” badge instead of pretending the noise is gone.
4. **The fixture app is a first-class scope.** This demonstrates the containment model: the
   same code that pollutes a whole-repo scan is coherent when isolated.
5. **Entry points are curated roots, handlers and top-level components.** 803 functions have
   zero *resolved* callers, but most are exported, dynamic, or test helpers. Listing all 803
   would repeat the flat-toolbar problem in a new place.
6. **`invoke` is the default function, and Story is the default view.** It is a real entry
   point with seven source-grounded steps, so the first screen shows the product at its best.
7. **270px sidebar / 320px drawer.** These are the exact widths you specified.
8. **The drawer starts open at ≥1280px and becomes an overlay below 1180px.** The canvas stays
   usable on laptop screens.
9. **Six steps visible, expandable.** `data.js` embeds up to 14 steps per function to keep the
   static payload at 3.2 MB; the real app should load steps on demand.
10. **Only three curated stories.** Hand-writing 1,068 narratives would invent behavior. The
    manifest fallback keeps the rest honest.
11. **Rescan, Open source and “live” mode are simulated.** There is no backend in this mock.
12. **Replaced the palette and type stack after the slop feedback.** The layout stays because
    you specified it; the visual language is now warm, flat and typographic instead of
    neon-on-slate.
13. **One accent, semantic colour only for state.** Vermilion/copper marks actions and the
    current selection; green, amber and red are reserved for return, warning and error.
14. **Language is metadata, not state.** All language badges are neutral; the rainbow chips
    were the loudest slop signal.
15. **No gradients, glows or heavy shadows.** Corners drop to 4–8px and the primary buttons
    are solid ink-on-accent blocks.
16. **Paper is the default, Graphite is the alternate.** Paper is the decisive departure from
    the old dark-indigo look; Graphite keeps a dark option without bringing the indigo back.

## Validation checklist

- [ ] **Scope isolation** — switch between all three scopes. Sidebar, breadcrumb, canvas and
      sandbox reset together. No function from one scope appears in another.
- [ ] **Default state** — opens on `fnscope-crates`, `invoke()` selected, Story view, Paper palette.
- [ ] **Palette** — toggle Paper/Graphite with the moon/sun button. Both stay readable, the
      choice survives a reload, and only actions/selection/current-state use the accent.
- [ ] **Entry points** — shows `invoke`, `main`, `read_manifest`, `router`, etc., not 803
      orphans. The footnote explains the hidden zero-caller functions.
- [ ] **Module tree** — expand `crates/fnscope-server` → `src/lib.rs`; select `router()`.
      Canvas, breadcrumb and selected row all follow.
- [ ] **Search** — `⌘K` / `Ctrl+K` or `/` focuses search. Search `token` in the full-tree scope;
      the fixture function appears with its module and file. Clear with the ×.
- [ ] **Breadcrumb** — on `invoke`, click `Inspect load_graph()`; the trail becomes
      `invoke() › load_graph()`. Back arrow returns. Clicking an earlier crumb truncates.
- [ ] **Story sections** — Triggered by, Inputs, Execution story, Returns, Error paths.
      Expand “Show more steps” on a long function.
- [ ] **1-Hop** — callers left, current function center, callees right; both sides clickable.
- [ ] **Split** — source on the left for curated functions, story on the right.
- [ ] **Raw** — full trimmed JSON record and Copy JSON.
- [ ] **Invoke** — open the drawer from the header card; edit arguments; Run function shows a
      simulated response and per-step trace. Invalid JSON shows an inline error. Live runtime
      tab explains that it is mocked. `⌘Enter` / `Ctrl+Enter` runs.
- [ ] **Keyboard** — `1`–`4` switch Story / 1-Hop / Split / Raw when not typing; `Esc` closes
      the project menu or drawer; `Alt+←` steps back in the trail.
- [ ] **Responsive** — narrow the window below 1180px: the drawer overlays with a backdrop and
      closes on backdrop click. Toggle the sidebar from the top-left button.
- [ ] **Console** — no errors during any of the above.

## What to challenge

- Is Paper or Graphite the better default? Is the vermilion/copper accent too strong or too quiet?
- Does IBM Plex + the serif wordmark feel authored, or still like a generated dev tool?
- Are the code blocks and 9–10px metadata readable in both palettes?
- Should this go further — fewer cards, denser terminal layout, or a more editorial canvas?
- Is `fnscope-crates` the right default, or should the first screen be the full tree?
- Is “entry point” the right filter, or should zero-caller functions be shown with an
  “unresolved callers” warning instead of hidden?
- Does the story order (callers → inputs → steps → outputs) match how you read a function?
- Is the 1-Hop layout useful, or should it be a true graph with edges?
- Does the drawer need to stay open while navigating, or close on selection?
- Are the 270/320 widths right in practice?

## Files

| File | Purpose |
| --- | --- |
| `index.html` | Shell: top bar, sidebar, canvas, drawer, toasts |
| `app.js` | State, rendering, navigation, views, sandbox, shortcuts |
| `data.js` | Generated prototype dataset (3.2 MB) |
| `generate-data.js` | Regenerates `data.js` from `.fnscope/fn-graph.json` |

Regenerate the data after a new self-scan:

```bash
node wireframe/fnscope-workspace/generate-data.js
```

## After approval

The real change becomes a feature pack (GRILL → PRD-TRD → IMPL) covering: a per-scope registry,
scope-aware server endpoints, the explorer/navigation/story UI, default exclusion of
`tests/fixtures/**`, and tests for cross-scope isolation. No app code is changed until you
validate this direction.
