# Brag Plan: Hitch

## What is this app?
Hitch is a Rust CLI that treats environment branches (`dev`, `qa`, `staging`) as
generated outputs — `base + ordered list of feature branches` — and rebuilds them
from that declaration instead of accumulating merges.

## The angle
Your `dev` branch isn't a history. It's an equation. Every team knows the pain of
"B is in dev but isn't ready for QA" — Hitch's answer is to stop merging and start
declaring. The video turns a tangled merge graph into one line of math, then shows
the CLI editing that math: demote a branch, hold a conflict, promote without
merging dev → qa.

## Hook (first 2-3 seconds)
A tangled merge-history graph of `dev` scribbles itself onto a dark terminal
canvas. One feature node, `B`, pulses red. Caption: **"B isn't ready for QA."**
then **"Now what?"** The pain is instantly recognisable to anyone who has run
`git revert -m 1`.

## Key moments (the middle)
- The graph collapses into the equation `dev = main + A + B + C`, with the
  tagline "Git environments as desired state, not merge history."
- `hitch demote B dev` types out; a real-format plan prints (Current / Proposed);
  `+ B` lifts out of the equation and `A + C` close the gap. "No need to undo the
  merge of B."
- A conflicting branch is **held**, not fatal: the real `⛔ clash held — conflicts
  with main` block with its `fix: git checkout clash && git rebase main` line, and
  the receipt headline "Applied, with branches held".
- Two equations stacked: `dev = main + A + B + C` / `qa = main + A + C` —
  "Hitch did not merge dev → qa."

## Outro / punchline
Hitch logo (the red hitch mark) + "hitch" wordmark + `cargo install hitch`.
Line: "Environment branches are outputs."

## User flow worth showing
1. Entry: an environment's declaration — `dev = main + A + B + C`.
2. Key action: `hitch demote B dev` → plan → receipt.
3. Result: `dev = main + A + C`; and on a conflict, the build still lands with the
   branch held and named.

## Tone
- Preset: default (leaning polished)
- Creative direction: terminal-native product film — the equation is the hero
- Interpretation: calm confident pacing, one dry joke (the tangled graph), clean
  monospace UI, soft crossfades/wipes; let real CLI output carry the scenes.

## Format: landscape — 1920x1080
## Duration: 21.5s

## Visual identity (from the project)
- Background: near-black terminal `#0e0d0d` (no web palette exists; CLI tool)
- Accent: Hitch logo red `#912e2e` (from `hitch.svg`), brightened to `#d0524f` for
  text-on-dark; secondary amber from the desktop brand gradient `#ffd60a → #ff9f0a`
- Text: `#f2efe9`, dim `#8a8580`
- Glyph colors (real CLI): ✓ green `#5fd38a`, ⛔ red, ⚠️ amber
- Display font: system sans (desktop app uses SF Pro / system-ui) → Inter fallback
- Body/code font: JetBrains Mono (monospace terminal)
- Strongest visual element: the equation line `dev = main + A + B + C` and the
  real plan/receipt output

## Share copy (draft)
Your dev branch shouldn't be a pile of merges. Hitch makes it an equation:
`dev = main + A + B + C`. Demote B, rebuild, done.

## Audio direction
- Role: warm bed with sparse professional accents
- Music: `happy-beats-business-moves-vol-11-by-ende-dot-app.mp3` (114.84 BPM)
- Music treatment: start at 0, quick fade-in (~0.3s), normal level, fade out over
  the last ~1.5s under the logo
- Music cue guidance: preset read
  (`assets/music/cues/happy-beats-business-moves-vol-11-by-ende-dot-app.music-cues.md`).
  Strong cues: 1.60s (hook "Now what?" / red pulse), 3.70s (graph → equation
  collapse), 17.91s (logo lands). Beat grid around 8.96–11.06 for the plan lines
  (hold the full plan after it prints).
- Audio-reactive treatment: subtle — the logo-red glow behind the equation breathes
  with RMS; no waveform bars
- SFX posture: sparse, motion-matched
- Audio-coupled moments: typed commands (soft key ticks), `B` lifting out (soft
  swoosh), held ⛔ (low muted thud), logo (one clean hit)
- Restraint rule: no harsh clicks over plan text, no stacked SFX; ticks quiet

## Storyboard

### Scene 1 — Tangled dev — 3.5s (0.0–3.5)
Dark canvas. A messy git graph draws itself: `main` line, feature lanes `A`, `B`,
`C` merging into `dev` with crisscrossing merge commits. Node `B` pulses red.
Text: "B isn't ready for QA." (settled ~1.4s) then "Now what?" (~0.9s, lands near
1.60s cue... or after first line; readability first).
Sequential/interaction: yes — graph edges draw in sequence; B pulse.
Audio intent: curious, slightly tense intro of the bed.
Audio-coupled idea: soft tick per merge node landing; low pulse on B.
Transition mood: dramatic → the graph collapses into a single line.

### Scene 2 — The equation — 3.5s (3.5–7.0)
Graph lines pull together into `dev = main + A + B + C` (huge monospace, centered;
`dev` in red accent). Below: "Git environments as desired state, not merge history."
(settled ≥2.4s). Small Hitch logo top-left.
Sequential/interaction: equation terms snap in one by one (beat-grid accents only;
headline appears once and holds).
Audio intent: release / resolve.
Audio-coupled idea: collapse locks to 3.70s strong cue.
Transition mood: clean → equation shrinks to top, terminal window slides up.

### Scene 3 — Demote — 4.5s (7.0–11.5)
Equation stays pinned at top. Terminal window types `$ hitch demote B dev`.
Plan prints (real format): "Demote B from dev / Current / dev = main + A + B + C /
Proposed / dev = main + A + C". Then receipt "Applied / ✓ dev". On apply the
pinned equation animates: `+ B` lifts out and fades, `+ C` slides left.
Caption: "No undoing merges. Just a rebuild."
Sequential/interaction: typed command; plan lines print sequentially, then hold.
Audio intent: confident, productive.
Audio-coupled idea: key ticks while typing; soft swoosh on B lifting out.
Transition mood: clean wipe → Scene 4.

### Scene 4 — Held, not broken — 4.0s (11.5–15.5)
Terminal shows the real `hitch rebuild staging` output excerpt:
```
Composition
  ⛔ clash held — conflicts with main
      shared.txt
    fix: git checkout clash && git rebase main
Applied, with branches held
  ✓ staging
```
Caption: "Conflicts get held — and named." Exit-code chip: `exit 2` "held a
conflicting branch" (small, real table entry).
Sequential/interaction: ⛔ block appears, then receipt line.
Audio intent: a beat of tension resolved.
Audio-coupled idea: muted thud on ⛔; gentle chime on ✓.
Transition mood: soft → Scene 5.

### Scene 5 — Promote, don't merge — 2.4s (15.5–17.9)
Two equations stacked: `dev = main + A + B + C` / `qa  = main + A + C`. Above: `$
hitch promote A qa` faint. Caption: "Hitch did not merge dev → qa." (strike through
an arrow `dev → qa`).
Sequential/interaction: qa line appears after dev line.
Audio intent: lift toward the outro.
Transition mood: dramatic → logo.

### Scene 6 — Outro — 3.6s (17.9–21.5)
Hitch red logo scales in at 17.91s strong cue. Wordmark "hitch". Line:
"Environment branches are outputs." Below: `cargo install hitch`.
Audio intent: payoff, then fade.
Audio-coupled idea: one clean logo hit; music fades under hold.

**Music mood for this video:** upbeat, warm, understated
**Audio summary:** bed enters with the tangled graph, resolves on the equation,
ticks along with the terminal, and lands one hit on the logo before fading.
