# Hyperframes Composition Brief: Hitch

## Objective
Create a short launch-style brag video for Hitch, a Git CLI for environment branches.

## Output
- Composition directory: `/Users/martin/Developer/hitch/brag-output/composition/`
- Rendered video: `/Users/martin/Developer/hitch/brag-output/brag.mp4`
- Format: landscape — 1920x1080, 30fps
- Duration: 21.5 seconds

## Source Material
- Project root: `/Users/martin/Developer/hitch`
- Primary files read: `README.md`, `CHANGELOG.md`, `hitch.svg`, `src/core/render.rs`
  (plan/receipt wording), `crates/hitch-desktop/src/globals.css`
- Product name: Hitch (wordmark lowercase `hitch` is fine — it's the CLI name)
- Tagline / strongest claim: "Git environments as desired state, not merge history."
- Key UI to recreate: the CLI's plan → receipt output in a terminal window, and the
  equation `dev = main + A + B + C`.
- Copy that must appear verbatim (re-casing / splitting OK):
  - `dev = main + A + B + C`
  - `qa  = main + A + C`
  - Git environments as desired state, not merge history.
  - `$ hitch demote B dev`
  - Plan text: `Demote B → dev` / `Current` / `  dev = main + A + B + C` /
    `Proposed` / `  dev = main + A + C`, then receipt `Applied` / `  ✓ dev`
  - Held-conflict excerpt (real `hitch rebuild staging` output from README):
    ```
    Composition
      ⛔ clash held — conflicts with main
          shared.txt
        fix: git checkout clash && git rebase main
    Applied, with branches held
      ✓ staging
    ```
  - Hitch did not merge dev → qa.
  - Environment branches are outputs.
  - `cargo install hitch`
  - Captions (framing, may be reworded lightly): "B isn't ready for QA." / "Now
    what?" / "No undoing merges. Just a rebuild." / "Conflicts get held — and named."
- Logo: `assets/img/hitch.svg` (single-color `#912e2e` mark, square viewBox 2346).

## Creative Direction
- Tone preset: default (leaning polished)
- Creative direction: terminal-native product film — the equation is the hero
- Interpretation: calm confident pacing, one dry joke (the tangled merge graph
  collapsing into one line), clean monospace UI, soft crossfades / clean wipes.
- Angle: Your `dev` branch isn't a history, it's an equation. Show the pain
  (tangled merge graph, B not ready), collapse it into `dev = main + A + B + C`,
  then show the CLI editing the equation.
- Hook: tangled `dev` merge graph draws itself, node `B` pulses red, "B isn't
  ready for QA." → "Now what?"
- Outro / punchline: red logo + `hitch` + "Environment branches are outputs." +
  `cargo install hitch`.
- Avoid:
  - Generic SaaS language
  - Abstract filler visuals (the git graph is the product's own concept, keep it
    recognisably a git graph: lanes, commit dots, merge curves)
  - Invented numbers, metrics, testimonials
  - Unrelated visual redesign

## Visual Identity
- Background: `#0e0d0d` (terminal near-black), subtle warm vignette ok
- Text: `#f2efe9`; dim `#8a8580`
- Accent: Hitch red `#912e2e` (logo) — use `#d0524f` for red text on dark
- Secondary accent: amber `#ff9f0a` (from desktop brand gradient `#ffd60a → #ff9f0a`), use sparingly
- CLI glyph colors: ✓ `#5fd38a`, ⛔ `#e5534b`, ⚠️ `#ff9f0a`
- Display font: Inter (system-ui feel; desktop app uses SF Pro)
- Code font: JetBrains Mono
- Visual references: terminal window with traffic-light dots, the equation line,
  the logo mark

## Storyboard
Use the storyboard in `/Users/martin/Developer/hitch/brag-output/brag-plan.md` as the creative contract.

1. Tangled dev — 0.0–3.5s — merge graph draws, B pulses red, "B isn't ready for QA." then "Now what?"
2. The equation — 3.5–7.0s — graph collapses to `dev = main + A + B + C`; tagline holds ≥2.4s
3. Demote — 7.0–11.5s — `$ hitch demote B dev` types; plan prints; receipt; pinned equation drops `+ B`
4. Held, not broken — 11.5–15.5s — real held-conflict excerpt; caption; small `exit 2` chip
5. Promote, don't merge — 15.5–17.9s — dev/qa equations stacked; "Hitch did not merge dev → qa."
6. Outro — 17.9–21.5s — logo lands on 17.91s cue; wordmark; tagline; `cargo install hitch`

## Audio
- Audio role: warm bed with sparse professional accents
- Audio arc: bed enters with the graph, resolves at the equation, ticks with the terminal, one hit on the logo, fades out under the final hold
- Music: `assets/music/happy-beats-business-moves-vol-11-by-ende-dot-app.mp3` (already copied)
- Music treatment: start 0, ~0.3s fade-in, fade out over final ~1.5s
- Music cue guidance: preset `/Users/martin/.claude/skills/brag/assets/music/cues/happy-beats-business-moves-vol-11-by-ende-dot-app.music-cues.json` (114.84 BPM). Strong cues: 1.60s, 3.70s (equation collapse), 17.91s (logo). Beat grid 7.38–11.06 for plan lines but hold full plan once printed.
- Audio-reactive treatment: subtle — red glow behind equation / logo breathes with RMS
- Audio-coupled moments:
  - Scene 1 — soft tick per merge node; low pulse on B
  - Scene 3 — key ticks while typing; soft swoosh as `+ B` lifts out
  - Scene 4 — muted thud on ⛔; soft chime on ✓
  - Scene 6 — one clean logo hit
- SFX selection guidance: SFX library at `/Users/martin/.claude/skills/brag/assets/sfx/` (folders: keyboard, ui, interface, impact, casino); read `sfx-analysis.md` there and prefer low high-frequency-risk files for repeated ticks.
- Exact SFX choice: Hyperframes chooses filenames, timestamps, density, volume.
- Audio files: copy chosen SFX into `composition/assets/sfx/`.

## Hyperframes Instructions
Use Hyperframes conventions (`npx hyperframes docs` for composition contract,
`data-*` timing, animation, keyframes, audio-reactive, CLI). Do not run the
generic intent interview / promo workflow.

Requirements:
- Show real CLI output (above) — it is the product.
- All text readable: ≥0.8s settled for labels, ~0.3s/word for sentences.
- 15–25s total (target 21.5s).
- Include music + sparse SFX.
- Major reveals may move to strong cues within ~0.15s; ignore cues that hurt readability.
- Every frame a pure function of time; wait for fonts.
- `npx hyperframes check` must pass with zero errors before render.
- Keep everything local; no publish.
