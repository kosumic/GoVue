# README previews

These are real GoVue screens captured on 2026-10-18 using a dedicated
**iPhone 17 Pro simulator running iOS 26.5**. The app was built in Release
configuration for the ARM64 simulator. Original captures are **1206 × 2622**
pixels. The gallery wraps them in the same iPhone frame used by Whip, producing
transparent **706 × 1466** lossless WebP images. The app screen keeps its aspect
ratio, actual status bar, and Dynamic Island.

The physical frame comes from `@sneas/telephone` 1.2.0. Its synthetic status UI
is removed; source, attribution, and the MIT license are in
[the frame directory](../frames/README.md).

The gallery follows Whip's two-column README layout. Its screens show:

| File | Screen |
| --- | --- |
| [browse.webp](browse.webp) | YouTube search results for Easy French's introduction lesson |
| [playback.webp](playback.webp) | Native playback of the French lesson with YouTube comments below |
| [transcript.webp](transcript.webp) | French auto-generated transcript with the current caption highlighted |
| [search.webp](search.webp) | French transcript search for “comment” to find questions to practice |
| [word-card.webp](word-card.webp) | Long-press card for “comment,” scrolled to its real AI usage example, English translation, and grammar explanation |
| [chat.webp](chat.webp) | A real GPT-6-Astra reply with informal and formal French questions and English translations |
| [languages.webp](languages.webp) | A sample profile with native English and French at A2 Elementary |
| [providers.webp](providers.webp) | Available AI account and API connection types, captured before sign-in |
| [download.webp](download.webp) | Video/audio download choices before starting a transfer |
| [extensions.webp](extensions.webp) | Grouped extension preferences |

Video frames, thumbnails, and captions come from Easy French's public lesson
[“How To Introduce Yourself in French | Super Easy French 196”](https://www.youtube.com/watch?v=tCd_4uCD5eo).
The captures use a sample language profile and no signed-in YouTube account.

The chat uses a signed-in ChatGPT / Codex connection with the French
auto-generated transcript attached. Account credentials were verified across app
restarts. YouTube returned HTTP 429 for the English machine translation during
capture, including one retry; the gallery shows French phrase search instead.

Apple's translation sheet reports that on-device translation is unsupported in
the simulator. The word-card capture shows its completed AI example and grammar
explanation after dismissing that notice and scrolling to those sections.
Word saving was also exercised through the card. These captures do not verify on-device word
translation; that needs a physical device.

## Replacing a capture

Build the app with the repository's iOS workflow on an Apple Silicon Mac, launch
it in a simulator, and navigate to the screen being replaced. Give loading and
scroll animations time to finish. Keep the keyboard and system dialogs closed.

Keep Xcode's simulator signing enabled for account captures. With `xcodebuild`,
use `CODE_SIGNING_ALLOWED=YES CODE_SIGN_IDENTITY=-` so Xcode embeds the simulator
application identity required by Keychain. Missing that identity causes
[`errSecMissingEntitlement` (`-34018`)](https://developer.apple.com/documentation/security/errsecmissingentitlement)
when GoVue reads or saves provider credentials.

For consistent status-bar and appearance settings:

```sh
SIMULATOR_UDID='your-simulator-udid'
xcrun simctl status_bar "$SIMULATOR_UDID" override \
  --time '9:41' --dataNetwork wifi --wifiMode active --wifiBars 3 \
  --batteryState charged --batteryLevel 100
xcrun simctl ui "$SIMULATOR_UDID" appearance dark
xcrun simctl io "$SIMULATOR_UDID" screenshot /tmp/govue-preview.png
```

Copy the original PNG to `.cache/readme-preview/raw/SCREEN.png` in the
repository. Keep these originals outside version control and separate from the
framed output. Generate all mockups with `resvg` and `cwebp` from the development
shell:

```sh
nix develop -c npm run preview:mockups
```

To replace one screen, pass the input directory and its base name:

```sh
nix develop -c npm run preview:mockups -- .cache/readme-preview/raw chat
```

The generator accepts only original 1206 × 2622 PNGs, retains them, and renders
all requested images before replacing the gallery files. It never adds a frame
to an already framed image.

Inspect the final image and update the gallery's caption and alternative text
when its content changes. These captures document simulator UI and the actions
observed during capture; the broader verification inventory remains in
[PORTING.md](../../PORTING.md).
