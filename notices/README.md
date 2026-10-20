# Third-party components

The isolated token renderer bundles [BgUtils](https://github.com/LuanRT/BgUtils). Its MIT notice is included here and in the generated JavaScript shipped with the app. The legacy YouTube.js decoder has been removed; its notice is retained for historical attribution.

GoVue is a React Native/Rust port of [HydeYYHH/Litube](https://github.com/HydeYYHH/Litube), based on commit `bb9ee78428a30347fc6eaf21a679f2a0b8cbc91f` (2026-10-16). The browser integration scripts under `src/youtube/litube/` are copied or adapted from LiTube, with the page bridge adapted to React Native. Player presentation and state are being ported to shared React Native and Rust. The shared vectors in `src/player/icons.tsx`, including the About mark, are adapted from the same revision’s Android drawables. LiTube's GNU GPL version 3 license is included in `LiTube-GPL-3.0.txt` and the repository `LICENSE`. Original MIT notices for dependencies remain applicable to those components.

SponsorBlock data comes from the [SponsorBlock API](https://wiki.sponsor.ajay.app/w/API_Docs). Requests use the hash-prefix endpoint, and the client only applies matching-video segments in enabled categories.

The Cast platform adapter ports LiTube's device authentication and framing helpers. Those helpers include NewTube (MIT) adaptations and Chromium (BSD) trust data; their notices are included in `NewTube-MIT.txt` and `Chromium-BSD.txt`. The Cast CRL trust anchor was regenerated from Chromium's certificate array, correcting an extra prefix byte in the reference copy. Receiver/session policy belongs in Rust; platform TLS and certificate verification stay in the native transport adapter.

The shared Cast browser player bundles dash.js 4.7.4 from the same LiTube revision. Its BSD license is in `Dash-js-BSD.txt`; the bundle's third-party notices remain beside it in `core/src/cast/assets/dash.all.min.js.LICENSE.txt`. The player and its scripts are served locally inside the explicitly shared session.

The Android build patches Expo Video's PiP activity lookup. Expo Video's MIT license is included in `Expo-Video-MIT.txt`.

The bundled language picker uses language codes and aliases from the [Library of Congress ISO 639 list](https://www.loc.gov/standards/iso639-2/ISO-639-2_utf-8.txt), and Unicode CLDR display names captured through `Intl.DisplayNames`. The Unicode data license is included in `Unicode-3.0.txt`.

OpenMinis was studied as an architectural reference for model providers, account connections and token-usage reporting, including the Android usage screens at commit `b4c0661d5631ebab4d1a2e6f3fd4c805d4030a6c`. No OpenMinis code was copied or translated. GoVue implements the provider protocols and usage aggregation independently in its shared Rust core with shared React Native presentation and native adapters. Identity-token verification uses jsonwebtoken; its MIT notice is included in `Jsonwebtoken-MIT.txt`.

AI replies and video summaries use [Enriched Markdown](https://github.com/software-mansion/enriched-markdown) for native Markdown rendering. Its license and the bundled parser, syntax-highlighting, Unicode, and math notices are included in `EnrichedMarkdown-ThirdParty.txt`.

Chat voice input uses [Expo Speech Recognition](https://github.com/jamsch/expo-speech-recognition). Its MIT license is included in `Expo-Speech-Recognition-MIT.txt`.
