# Reader, vocabulary, and chat

[Back to GoVue](../README.md)

The player's **Transcript** and **AI** buttons appear before speed and quality.
Transcript opens the reader; AI opens the same full-screen conversation workspace
as **Chat** in the bottom navigation.

Opening a video starts loading its transcript in the background. The reader
stays closed until you tap **Transcript**, then reuses the loaded captions or the
request already in progress. Closing the reader keeps that work available for
the current video; opening another video cancels the previous request.

## Read and compare captions

The reader supports language selection, local search, playback following, and
tap-to-seek captions. YouTube JSON3 word timings are preserved when available:
the current word is highlighted, and tapping a word seeks to its start. Captions
without word timings use the line start. Refresh an older cached transcript to
fetch word timings.

Select up to two transcript languages to compare captions interleaved in timestamp
order, even when the languages split lines differently. Search matches either
language and keeps the paired lines together; copy and share include both selected
languages. Tap a selected language to remove it.

The reader keeps original line breaks, reconstructs continuations within caption
windows, and displays authored emphasis and Japanese pronunciation readings.
Readings are searchable and included when copying or sharing. Subtle dividers
mark possible speaker changes when the caption track supplies those flags,
including changes within a caption; they do not identify speakers.

**Translate…** offers YouTube's available machine translations with their source
language. Default caption selection follows the selected audio track when YouTube
supplies the association. **Caption details** shows source structure, audio
associations, recognition scores, and speech-boundary statistics. These scores
are not calibrated accuracy percentages, and speech-boundary hints do not
identify speakers.

Validated cue fragments, timing, formatting, window definitions, confidence,
speech-boundary hints, and translation/audio metadata stay in the native caption
cache. Upstream addresses, tokens, and raw responses are excluded. See
[standalone extraction](extraction.md) for the current extraction path.

## Translate and save words

Set your known and learning languages in **Extensions → Languages**. Known
languages record your current level, including a native language; learning
languages record your target level. A language can appear in both lists.

Hold a transcript word to open a floating card near it and translate on device,
then optionally save it with that timestamp. Words in a learning language
translate into a language you know, preferring your native language. Words in a
known language translate into all your learning languages, with a separate
result for each. Languages outside your profile use a known language, or English
when none is available.

The card can generate a usage example with translations using your selected AI
model, or explain the word's base form and grammar in its current caption, in
your known/native language. Each example sentence and translation has a speaker
button for text-to-speech in its own language.

If the word has individual timestamps, **Play word** plays its original audio
and stops at the word's end. It shares the reader and games' lazy video-caption
audio cache; words in the same caption reuse downloaded fragments.

The first translation may download a language model. Android uses ML Kit through
`expo-translate-text`; iOS translation requires iOS 18 or newer. Rebuild the native
app after adding or changing the translation module.

## Open a video conversation

Opening **AI** from a video reopens its saved conversation, or creates an empty
chat with the transcript as context. Opening chat never sends a message or starts
a summary. Choose a model from the header and send your own question; model
changes keep the video's conversation.

Use **Open video** in chat to return to its existing video tab or open it again.
Tap the transcript context card to read or search the captions used by the AI.
Tap a line to hear its original audio and stop at the end of that caption. Audio
loads on demand and is cached locally for replay and reuse by chat games.

Reasoning changes made during a response apply to the next message. Each response
has a regenerate button beside its copy action. Regenerating an earlier response
replaces that response and all later messages using its original prompt.

You can send another message while the assistant is working. It appears in a
queue and steers the current work after the next batch of tool results, before
the model continues. If the assistant finishes without a tool call, queued
messages are delivered then. Messages queued together are combined in order.
Stop remains available beside Send. Stopping or a failed request keeps undelivered
messages visible; tap one to send it when the assistant is idle.

For playable learning activities, ask the assistant to create a game. Game
interfaces can use selected captions and original audio, with progress saved by
Rust. See [interactive chat games](interactive-games.md) for the runtime contract
and audio limits.

Open **Practice** in the conversation sidebar to swipe through saved games:
left for **Forgot**, right for **Know**. Activities retain their own interactions,
and Undo restores the previous swipe. Progress survives app restarts and chat
deletion. Activities prepare their audio asynchronously when opened and can link
back to the original YouTube video and caption timestamp.

## Find and organize chats

Open the conversation drawer for saved chats, new chats, providers, and token
usage. Search the drawer by conversation title or message text; message matches
open the conversation at the matching message.

Use the magnifying glass in an open chat to find text, see a highlighted excerpt,
and move through matches with the previous/next arrows. Searches are local,
case insensitive, and cover user and assistant messages. Find also includes the
current streamed reply and shows up to 500 matches; narrow the query for more.

Long press a conversation to pin, archive, rename, or delete it. Pinned chats
stay at the top. **Archived chats** lets you browse, search, and restore archived
chats.

## Configure providers and instructions

Open **Extensions → AI** to manage providers, accounts, models, and model groups.
Choose **ChatGPT / Codex** for ChatGPT account sign-in or **OpenAI API** for an API
key. The alternative **ChatGPT plan sharing** connection is under **Advanced
connections**. Provider eligibility and model access depend on the provider.

Paste or write **AGENT.md instructions** there to set your preferred language,
learning level, or response style for all chats and summaries. Save to apply them
to the next message, including in existing chats. Clear and save to remove them.

## Understand usage and caching

Choose **Token usage** in AI settings for saved-chat totals, provider/model
breakdowns, cache hit rates, and output speed. Open a conversation for its context
and reasoning details. Missing provider counts are shown as unavailable; older
replies may lack cache, timing, and completion-date metadata. These are local
saved-history statistics, not the provider's billing or subscription quota.

Unreported cache counts appear as unknown, and cache hit rates use only replies
with both input and cache counts. Earlier request messages keep their original
normalized text; unchanged transcripts are attached once. Provider tool and
reasoning items are replayed from a bounded memory cache and never saved to disk.
Restarting the app or evicting that cache falls back to normalized chat history
and may start a new provider cache prefix.

Both ChatGPT account connections keep a stable conversation identity in the
prompt-cache key and session headers across turns and tool calls.

## Dictate a question

Tap the microphone and speak. Partial results appear separately, and only final
results are added to the current draft. Rust owns dictation state and joins
recognized text with spacing appropriate for English, Chinese, and Japanese.

Recognition uses the device locale and up to 100 vocabulary hints built in Rust
from the draft, recent chat messages, and the attached transcript. Hint loading
falls back to draft vocabulary after 1.5 seconds. Android finishes on a pause;
iOS supports continuous dictation until stopped. GoVue does not save audio
recordings.

Voice input uses `expo-speech-recognition`. Rebuild the native app after adding
or changing the dependency so the speech module and microphone permissions are
included.

## Listen to a response

Each assistant response has a **Read** button in its footer. It reads the response
with the device's default text-to-speech voice and becomes **Stop** during playback.
Reading pauses video and caption audio, skips Markdown formatting and link
targets, and stops when you leave the conversation, background the app, or start
dictation. Long responses are split into speech requests by the Rust core.
