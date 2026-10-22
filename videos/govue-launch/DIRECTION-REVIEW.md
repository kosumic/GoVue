# Direction review after the rejected first cut

**Current revision:** The user also rejected the phrase-panel treatment below.
The active v3 film uses fresh, moving GoVue screen recordings on Whip's licensed
3D iPhone model. See STORYBOARD.md for its camera moves and live interactions.
The following notes describe the earlier direction and are retained as history.

User feedback: "still looks like a slop. Can you check github skill to make cool vidoes"

The requested deliverable remains a 45-second TikTok launch video with cool 3D
motion graphics. Preview before building the full replacement or exporting.

## Sources inspected

The repositories were shallow-cloned under ~/repos after checking for existing
clones. The existing HyperFrames checkout was refreshed. No third-party setup
scripts were run and the video framework remains HyperFrames.

| Source | Inspected revision | Useful material | Decision |
| --- | --- | --- | --- |
| [iart-ai/motion-design-skills](https://github.com/iart-ai/motion-design-skills) | 3c129f7 | motion-art-direction, shot-composition, animation-principles, beat-sync-editing, direction-playbook | Main creative guidance. Establish one hero, a motion language, and a restraint pass before animation. |
| [heygen-com/hyperframes-launches](https://github.com/heygen-com/hyperframes-launches/tree/main/heygen-apple-motion) | d7ac350 | Finished re-brandable launch sources; 02-bouncy-ui, 04-generate-reel, their README files and cut ledger | Main implementation study. Preserve an object's lineage and direction of travel across cuts. Study geometry and timing; replace brand assets with GoVue's own. |
| [KJ-AIML/product-launch-film](https://github.com/KJ-AIML/product-launch-film) | 6dbfb49 | Anti-slop checklist and Connectra/Heli revision case studies | Supporting critique. Prefer a concrete learning scenario, fewer elements, and genuine product evidence. Its Remotion starter does not replace the requested HyperFrames project. |
| [memex-lab/product-launch-video-skill](https://github.com/memex-lab/product-launch-video-skill) | 9de027d | Full launch workflow, storyboard schema, contrast in pacing and shot scale | Secondary reference. Avoid mechanically following its ambient-layer prescriptions; that can recreate the current visual clutter. |

These are source-level findings, not a claim that installing a skill guarantees
good art direction. Some finished-example media are Git LFS pointers in the text
checkout; those videos have not been watched from the local clone.

## Why the current cut fails

- Six beats reuse the same silhouette: a heading in the upper third and a small
  phone below. Changing the screenshot does not create a new shot.
- Chrome rings, ribbons, floating tiles, and the phone compete for attention.
  The rings have no relationship to learning a language.
- Most movement is decorative orbiting. The viewer does not see an action cause
  a useful outcome, such as selecting a phrase and hearing it again.
- The captions, selection, and quiz are miniature inside screenshots. They need
  close, readable product views.
- Generic slogans carry the story while the actual learning experience stays
  secondary. The large extruded GoVue ending makes the geometry the subject.
- A passing technical check proves the composition works, not that it looks good.

## Recommended replacement: follow one phrase

One concrete French lesson carries the film: **Comment tu t'appelles ?**
The exact source phrase already appears in the app captures. The viewer follows
it from the video, through selection and understanding, into practice.

Use dimensional motion to connect those states. A selected caption becomes the
same physical phrase surface seen from another angle; that surface opens into
the learning activity. Keep the screen or selected phrase as the hero. Remove
the rings, decorative play sculptures, ribbons, and oversized toy-like wordmark.

### Motion language

| Property | Choice |
| --- | --- |
| Mood | Curious, tactile, energetic |
| Easing | One exponential deceleration family; mirrored acceleration for exits |
| Timing unit | 0.25 seconds; 1–1.5-second readable holds after key reveals |
| Transitions | Object match cuts with matched position and movement; no montage of unrelated spinning props |
| Camera | One deliberate move per beat: macro push, half-turn, or pull-back |
| 3D hierarchy | The caption or real app surface is primary; lighting/shadow responds to it; the background stays still |
| Palette | A light studio field with dark type and GoVue blue; actual dark app UI stays authentic |
| Restraint | No chrome rings, constant drift, particles, generic cards, permanent chapter labels, or floating phone fan |

### Three keyframes to review before the full rebuild

1. **The phrase.** A close, readable French caption from the actual lesson, with
   its selected word as the only blue focus. No phone shell or sales headline.
   A single forward camera move leads toward that word.
2. **The meaning.** The same selected phrase forms a physical surface and turns
   in depth to reveal its English meaning. The real phrase-help screen supplies
   evidence; the translation carries the frame.
3. **Your turn.** The phrase surface opens into the actual introduction quiz.
   The activity is large enough to read, with one clear question and the lesson's
   audio affordance. The cut preserves the prior surface's visual position.

The user's subsequent “improve it” instruction authorized the replacement preview.
The phrase-led version is now implemented in stage-v2.js and compositions/v2/.
STORYBOARD.md records its complete 45-second timeline. The rejected version's
source remains available in versions/v1/source.json. Delivery export still waits
for the user's preview review.
