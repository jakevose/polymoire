# Polymoire

A generative music visualizer (Rust, egui/wgpu, cpal). Start here:

1. Read `docs/HANDOFF.md`: current state, design decisions and why, tuning dials,
   open items.
2. `README.md` is the user-facing description; keep its plain-language style.

## Keep the handoff notes current

Update `docs/HANDOFF.md` in the same commit whenever a session changes behavior, makes a
design decision, tries and drops an approach, gets feedback from the user on how things
feel, or opens or closes an item:

- Refresh "Where things stand", the dials and the probe baseline if they changed.
- Move finished items out of "Open items" and add new ones.
- Add a short dated line to the session log.

Record *why* and *what's next*, not what the code already says. Keep it short enough to
read in two minutes.

## Working conventions

- Run `mise run ci` (formatting check, clippy with warnings as errors, tests) before
  committing.
- Commit on `main`, which is the user's workflow. Push only when asked.
- The comfort rules (no flashing; whole-view camera moves only at half tempo; Reduce
  motion) are hard requirements, enforced by tests in `src/reactions.rs`. Never loosen
  them to make something feel livelier.
- Measure reaction tuning with `cargo run --release --example pulse_probe` before and
  after. Don't tune by guesswork.
- `mise run smoke` plays audio on the speakers. Ask before running it.
