# GoVue — Live on iPhone

35 seconds · 1080 × 1920 · HyperFrames · live iOS screen recordings.

The current film uses the same licensed iPhone 17 Pro model as Whip’s TikTok
film. Its display plays fresh recordings from GoVue running on mini’s dedicated
iPhone simulator. A rear-camera macro turns into the live display; the camera
reaches bilingual captions after four seconds, demonstrates a real caption
long press and AI chat, and closes on real quiz feedback and a front/back phone pair.

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

The playable review draft is renders/govue-v4-review.mp4. It is a 24 fps
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

The committed footage and soundtrack are sufficient to rebuild the preview.
To refresh them, record with scripts/record-live.mjs on mini, copy the raw MOVs
into the repository’s ignored .cache/, update capture/edits.json, and run
`npm run prepare:footage`, followed by `npm run build`. FFmpeg and FFprobe
must be available for footage preparation; use a Nix shell when needed.
capture/footage.json records source windows, speed, dimensions, and checksums.
Chat has a small timing adjustment; the long-press capture plays at real speed.

`npm run build:audio` reconforms the licensed source track using FFmpeg.
See ASSETS.md and assets/audio/TECH-HOUSE-VIBES-LICENSE.md for provenance.
The model’s required credit appears in the closing frame.

The rejected v1/v2 sources remain in versions/. Their still-image assets and
older composition files are retained for comparison; the active build is v3.
