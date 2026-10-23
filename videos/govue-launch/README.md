# GoVue — Live on iPhone

35 seconds · 1080 × 1920 · HyperFrames · live iOS screen recordings.

The current film uses the same licensed iPhone 17 Pro model as Whip’s TikTok
film. Its display plays fresh recordings from GoVue running on mini’s dedicated
iPhone simulator. A rear-camera macro turns into the live display; the camera
reaches bilingual captions after four seconds, demonstrates a real caption
long press and AI chat, and closes on real quiz feedback and a front/back phone pair.
The closing frame reads “Learn from every video lesson.” and includes Android
and iOS platform icons beside their labels.

```sh
cd videos/govue-launch
npm ci
npm run build
npm run check
npm run dev
```

Review at http://localhost:3077/#project/govue-launch. The preview is editable in
Studio. Delivery export still requires the user’s preview review; after approval,
`npm run render` writes renders/govue-tiktok-35s.mp4.

The older review draft, renders/govue-v4-review.mp4, predates this footage refresh. It is a 24 fps
preview; final delivery remains 30 fps. The review file was captured on mini's
Metal GPU with rasterScale set to 1, including full-resolution PBR materials.
On this workstation, use
`bash scripts/preview-browser.sh` to open Studio in an isolated software-WebGL
browser. The hardware driver can lose the 3D context. scripts/hf.sh defaults
CLI captures and checks to software WebGL for the same reason.
Interactive Studio uses a 540 × 960 3D canvas and simpler lit preview
materials beneath full-resolution type. Review and delivery commands set rasterScale
to 1 for a 1080 × 1920 3D canvas, PBR materials, and antialiasing. Geometry,
cameras, recordings, and timings are identical.

scripts/build-v3.mjs generates copy and the composition hierarchy. stage-v3.js
owns the phone, materials, camera, and video-texture updates. HyperFrames owns
all media playback, seeking, and extracted export frames.

Playback, reader, word-translation, and quiz footage were refreshed from GoVue 0.3.0
on 2026-10-22. The translation take uses the explicit recording mock described below.
The existing chat clip is retained at the user’s request.
The committed footage and soundtrack are sufficient to rebuild the preview.
To refresh them, record with scripts/record-live.mjs on mini, copy the raw MOVs
into the repository’s ignored .cache/, update capture/edits.json, and run
`npm run prepare:footage`, followed by `npm run build`. FFmpeg and FFprobe
must be available for footage preparation; use a Nix shell when needed.
capture/footage.json records source windows, speed, dimensions, and checksums.
The retained chat has a small timing adjustment. The physical long press plays
at real speed and opens the app’s translation card with a mocked result. The quiz
holds its final feedback screen where simctl omitted the unchanged recording tail.

For simulator recording, `EXPO_PUBLIC_GOVUE_MOCK_TRANSLATION=1` enables the small
fixture in src/native/translation.ts: French “apprendre” returns English “learn”.
Other word/language pairs report a missing fixture. The language settings are
English (native) under known languages and French (A2) under learning languages.
This stages the translation result because Apple translation is unavailable in
iOS Simulator; selection, the card, and the gesture remain the app’s actual UI.
From the repository root on mini, build the opt-in Release demo with:

```sh
EXPO_PUBLIC_GOVUE_MOCK_TRANSLATION=1 npm run ios -- --configuration Release --device 'GoVue README Preview'
```

Rebuild without that environment variable for normal translation. The mock is
disabled by default and requires no dependency or native source patches.

`npm run build:audio` reconforms the licensed source track using FFmpeg.
See ASSETS.md and assets/audio/TECH-HOUSE-VIBES-LICENSE.md for provenance.
The model’s required credit appears in the closing frame.

The rejected v1/v2 sources remain in versions/. Their still-image assets and
older composition files are retained for comparison; the active build is v3.
