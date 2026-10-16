# Third-party components

The injected player decoder bundles [YouTube.js](https://github.com/LuanRT/YouTube.js) and [BgUtils](https://github.com/LuanRT/BgUtils). Their MIT notices are included here and in the generated JavaScript shipped with the app.

Litube was used as an implementation reference. Its Kotlin downloader, Android player, and GPL JavaScript were not copied into this project. LingoPlay implements the corresponding behavior using its shared React Native/Rust architecture and the upstream libraries above.

SponsorBlock data comes from the [SponsorBlock API](https://wiki.sponsor.ajay.app/w/API_Docs). Requests use the hash-prefix endpoint, and the client only applies matching-video segments in enabled categories.

The Android build patches Expo Video's PiP activity lookup. Expo Video's MIT license is included in `Expo-Video-MIT.txt`.

OpenMinis was studied as an architectural reference for model providers and account connections. No OpenMinis code was copied or translated. LingoPlay implements the provider protocols independently in its shared Rust core and native adapters. Identity-token verification uses jsonwebtoken; its MIT notice is included in `Jsonwebtoken-MIT.txt`.

AI replies and video summaries use [Enriched Markdown](https://github.com/software-mansion/enriched-markdown) for native Markdown rendering. Its license and the bundled parser, syntax-highlighting, Unicode, and math notices are included in `EnrichedMarkdown-ThirdParty.txt`.
