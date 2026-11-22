import Foundation
import WebKit

@_cdecl("govue_ios_clear_youtube_cache")
func govueIOSClearYouTubeCache() {
    WKWebsiteDataStore.default().removeData(
        ofTypes: [WKWebsiteDataTypeMemoryCache, WKWebsiteDataTypeDiskCache],
        modifiedSince: .distantPast,
        completionHandler: {}
    )
}
