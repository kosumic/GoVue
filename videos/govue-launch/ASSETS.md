# Asset provenance — current revision

Playback, bilingual reader, long-press/translation, and quiz textures were refreshed on
2026-10-22 from the reinstalled GoVue 0.3.0 release (commit 05cabec,
io.github.kaminarios.govue). These are unframed 1206 × 2622 recordings from
mini’s dedicated iPhone 17 Pro simulator, running iOS 26.5. Capture uses simctl
and Whip’s existing AXe controller. The existing saved-conversation clip is
retained at the user’s request; the new app needs a fresh provider sign-in.
The quiz reuses the existing saved activity and records actual answer feedback.

capture/edits.json records the source windows; capture/footage.json records
dimensions, 30 fps conformity, speed, and SHA-256 checksums. Raw MOVs and action
logs are kept in the ignored repository .cache/. The retained AI-sheet take has
a small timing adjustment. The word clip uses the app’s opt-in translation mock
and holds its unchanged final card for readability. The quiz holds its unchanged final feedback screen to
fill the ten-second slot: simctl omitted the static tail of the 30-second take.
Playback, caption motion, and the physical long press play at their recorded
speed. These holds are explicit in the edit and footage manifests.

The lesson is Easy French’s
[How To Introduce Yourself in French | Super Easy French 196](https://www.youtube.com/watch?v=tCd_4uCD5eo).
Its images and subtitles are visible inside the recorded app. The soundtrack
does not include the lesson’s audio.

The iPhone 17 Pro model is by **Ibrahim.Bhl**, under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
[Original model](https://sketchfab.com/3d-models/iphone-17-pro-4aeeeb41f9d14f96bb3f2589edc3edac).
It is reused from Whip’s local launch project, scaled and lit, with the recorded
display added as a separate mesh. Its license is shipped in
assets/models/iphone-17-pro/LICENSE.txt, and its credit appears in the film.

Music: **Tech House vibes** by **Alejandro Magaña (A. M.)**, Mixkit catalog 130,
under the [Mixkit Stock Music Free License](https://mixkit.co/license/#musicFree).
[Official source download](https://assets.mixkit.co/music/130/130.mp3).
The local source is conformed from seconds 0–45 to −15 LUFS with a −1.5 dB
true-peak target and boundary fades. Its source checksum and license record are
in assets/audio/TECH-HOUSE-VIBES-LICENSE.md. The active composition uses 0–35
seconds, with a framework volume fade over its final 1.2 seconds.

The GoVue mark is the unmodified assets/icon.png from this repository. It appears
on the white 3D badge at the opening and closing. Three.js 0.181.2 (MIT), GSAP 3.14.2
(Standard License), Barlow (SIL OFL), and fontkit 2.0.4 are local and pinned.
Font outlines generate the extruded GoVue geometry. License files accompany the
vendor assets and fonts. The older model, screenshots, serif font, and original
synthesized score belong to the preserved rejected cuts and are unused in v3.

The closing Android and Apple icons come from Simple Icons (CC0 1.0), with their
paths preserved and their fill set to the closing frame’s light text color. The
Apple mark is labeled iOS. Pinned source URLs, checksums, and the source license
are included in assets/platforms/SOURCES.md and assets/platforms/LICENSE.md.

The long-press take uses a GoVue 0.3.0 Release build on the same dedicated
simulator, with `EXPO_PUBLIC_GOVUE_MOCK_TRANSLATION=1` enabled at the user’s request.
A real hold selects “apprendre”. The fixture in src/native/translation.ts returns
“learn” into the actual translation card because Apple translation is unavailable
in iOS Simulator. Known language is English (native); learning language is French (A2).
The translated text is staged; this take does not verify Apple’s translation engine. The
registry touch-indicator’s contact/lift envelope is adapted to the 3D glass and
synchronized to the recorded hold.
