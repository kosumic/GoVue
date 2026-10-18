# Interactive chat games

The assistant can call `create_interactive_game` to create a playable HTML card
in its reply. There is no registry of game types. The model supplies HTML, CSS,
presentation JavaScript, and declarative rules interpreted by Rust.

Open a card with **Play game**, or **Continue game** after making progress. Only
one card runs at a time. Expand it for more space; closing, expanding, or reloading
an interface preserves its Rust state. Game audio pauses the main video and stops
when the interface closes or the app enters the background.

## Definition

The tool accepts `title`, `html` (a body fragment), `css`, `script`, `rules` (a
JSON-encoded string), and `cueIds` (an array). Transcript context labels each
caption `cue-N`, where N is its zero-based normalized caption index. Selected
caption text and timing are copied into the saved game, independently of the
bounded transcript cache.

Use semantic buttons, visible labels, and the theme variables `--background`,
`--ink`, `--muted`, `--accent`, and `--raised`. The inline viewport is 400px high
and scrolls internally. Use `textContent` for caption and user text.

```js
game.onState(state => {
  document.querySelector('#answer').textContent = state.selected.join(' ');
});
document.querySelector('#choice').onclick = async () => {
  try { await game.dispatch('choose', { id: 'choice-1' }); }
  catch (error) { document.querySelector('#error').textContent = error.message; }
};
document.querySelector('#listen').onclick = async () => {
  try { await game.playAudio({ cueId: 'cue-42' }); }
  catch (error) { document.querySelector('#error').textContent = error.message; }
};
```

`onState` calls back immediately and on every subsequent host update. `dispatch`
resolves to the saved state. `playAudio` replaces any pending or playing clip
and resolves when playback starts. Keep audio cards enabled so another tap can
replace the current clip. Preparation failures reject the Promise; the host shows
any later playback failures. `game.cues` contains the selected captions as
`{ id, text, start, duration }`. Scripts render state and handle DOM interactions;
Rust owns durable transitions and scoring.

## Rules

```json
{
  "initialState": { "selected": [], "correct": false },
  "actions": [
    {
      "name": "choose",
      "when": { "op": "not", "args": [
        { "op": "contains", "args": [{ "state": "selected" }, { "input": "id" }] }
      ] },
      "updates": [{ "key": "selected", "value": {
        "op": "append", "args": [{ "state": "selected" }, { "input": "id" }]
      } }]
    },
    {
      "name": "check",
      "updates": [{ "key": "correct", "value": {
        "op": "eq", "args": [{ "state": "selected" }, { "literal": ["b", "a"] }]
      } }],
      "notify": true
    }
  ]
}
```

An expression is `{ "literal": value }`, `{ "state": "key" }`,
`{ "input": "key" }`, or `{ "op": operation, "args": [expressions] }`.
Keys address top-level fields; `get` reads deeper collections. Updates run in
order against the evolving state. `when` must evaluate to `true`.

| Operation | Arguments | Result |
| --- | --- | --- |
| `eq`, `lt`, `add` | Two values | Equality, numeric comparison, addition |
| `and`, `or`, `not` | Two booleans / one boolean | Boolean result |
| `length`, `last` | Collection / array | Length / final item |
| `get` | Collection, index or key | Item, or null when missing |
| `contains` | Array, value | Membership |
| `append`, `remove`, `concat` | Array, value or array | New array |
| `slice` | Array, start, exclusive end | New array |
| `if` | Condition, true branch, false branch | Lazily evaluated branch |

Use guards to validate choice IDs and prevent submitting incomplete answers. Give
repeated word tiles separate IDs. Include undo/reset actions and understandable
feedback. Unsupported mechanics need another generic Rust operation, rather than
a game-specific native component.

Actions are serialized, revision-checked, deduplicated by event ID, and saved
atomically with their game. Audio requests are recorded as `audio_requested`.
`notify: true` queues an assistant response after any current response finishes.
Every new model turn includes the latest games' states, selected captions, and
recent action histories as untrusted reference data. The interacted game is
prioritized; older events are compacted when the context budget is reached.
The creation tool completes
immediately; user interaction never leaves a model tool call open.

## Audio and runtime boundaries

Audio is fetched on the first play request. Rust selects indexed AAC audio,
downloads its initialization and caption-overlapping fMP4 segments using bounded
requests, and serves a local HLS clip to Expo Video. Subsequent plays use the
cached bytes, including after restart. Caption start/end offsets limit playback;
the cache includes the complete overlapping media segments. Current captions
provide line-level timing, so games should use short spoken phrases.

The cache is limited to 64 MB, each clip to 12 MB, and each caption to 30 seconds.
Active clip routes protect cached files from eviction. Routes use opaque random
IDs, bind to loopback, and are revoked after use. Signed addresses stay in memory.

Generated content runs in an opaque-origin iframe with `sandbox="allow-scripts"`
inside a dedicated WebView. The trusted outer document keeps its bridge session
private and validates the sending frame. Child-frame messaging globals are
hidden, and the native handler rejects messages without the current session.
CSP blocks external resources, media, connections, frames and workers; navigation,
file access, popups, cookie sharing and DOM storage are disabled. The generated
interface receives only game state, selected captions, and action/audio methods.
All bridge messages and rule expressions remain untrusted and bounded in Rust.
Documents, state, rules, event counts and expression evaluation have independent
size/depth/work limits. Game failures expose native close/reload controls.
