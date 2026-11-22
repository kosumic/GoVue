import UIKit
import WebKit

typealias GameEvent = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>, UnsafePointer<CChar>) -> Void

private final class GameRenderer: NSObject, WKNavigationDelegate, WKScriptMessageHandler {
    private weak var view: UIView?
    private let context: UnsafeRawPointer
    private var emit: GameEvent?
    private var web: WKWebView?
    private var generation: UInt64 = 0
    private static let messages = "govueGame"

    init(view: UIView, context: UnsafeRawPointer, emit: @escaping GameEvent) {
        self.view = view
        self.context = context
        self.emit = emit
    }

    func load(_ html: String) {
        retire()
        guard let view, !html.isEmpty, html.utf8.count <= 1_000_000 else {
            failure()
            return
        }
        let generation = self.generation
        let content = WKUserContentController()
        content.add(self, name: Self.messages)
        content.addUserScript(WKUserScript(
            source: "window.GoVueGame = Object.freeze({postMessage: value => window.webkit.messageHandlers.govueGame.postMessage(value)});",
            injectionTime: .atDocumentStart, forMainFrameOnly: true))
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.userContentController = content
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        config.mediaTypesRequiringUserActionForPlayback = .all
        let web = WKWebView(frame: view.bounds, configuration: config)
        web.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        web.isOpaque = false
        web.backgroundColor = .clear
        web.navigationDelegate = self
        view.addSubview(web)
        self.web = web

        let rules = """
            [
              {"trigger":{"url-filter":"^https?:"},"action":{"type":"block"}},
              {"trigger":{"url-filter":"^wss?:"},"action":{"type":"block"}},
              {"trigger":{"url-filter":"^ftp:"},"action":{"type":"block"}},
              {"trigger":{"url-filter":"^file:"},"action":{"type":"block"}}
            ]
            """
        WKContentRuleListStore.default().compileContentRuleList(forIdentifier: "govue-game-local", encodedContentRuleList: rules) { [weak self, weak web] list, _ in
            DispatchQueue.main.async { [weak self, weak web] in
                guard let self, let web, self.web === web, self.generation == generation else { return }
                guard let list else {
                    self.failure()
                    return
                }
                web.configuration.userContentController.add(list)
                web.loadHTMLString(html, baseURL: URL(string: "about:blank"))
            }
        }
    }

    func reply(_ value: String) {
        guard let web, value.utf8.count <= 100_000,
              let quoted = try? JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed),
              let source = String(data: quoted, encoding: .utf8) else { return }
        let safe = source.replacingOccurrences(of: "\u{2028}", with: "\\u2028")
            .replacingOccurrences(of: "\u{2029}", with: "\\u2029")
        web.evaluateJavaScript("window.__lingoGameReply?.(JSON.parse(\(safe)))")
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let web, message.webView === web, message.name == Self.messages, message.frameInfo.isMainFrame,
              message.frameInfo.request.url?.absoluteString == "about:blank",
              let raw = message.body as? String, raw.utf8.count <= 10_000 else { return }
        event("message", raw)
    }

    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let address = action.request.url?.absoluteString
        decisionHandler(webView === web && (address == "about:blank" || address == "about:srcdoc") ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
        if webView === web && (error as NSError).code != NSURLErrorCancelled { failure() }
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        if webView === web { failure() }
    }

    private func failure() {
        retire()
        event("failed", true)
    }

    private func event(_ name: String, _ value: Any) {
        guard let emit, let data = try? JSONSerialization.data(withJSONObject: value, options: .fragmentsAllowed),
              let raw = String(data: data, encoding: .utf8) else { return }
        name.withCString { name in raw.withCString { emit(context, name, $0) } }
    }

    private func retire() {
        generation &+= 1
        if let web {
            web.navigationDelegate = nil
            web.configuration.userContentController.removeScriptMessageHandler(forName: Self.messages)
            web.stopLoading()
            web.removeFromSuperview()
            self.web = nil
        }
    }

    func close() {
        retire()
        emit = nil
    }
}

@_cdecl("govue_ios_game_create")
func govueIOSGameCreate(_ view: UnsafeRawPointer, _ context: UnsafeRawPointer, _ emit: @escaping GameEvent) -> UnsafeMutableRawPointer {
    let view = Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue()
    return Unmanaged.passRetained(GameRenderer(view: view, context: context, emit: emit)).toOpaque()
}

@_cdecl("govue_ios_game_load")
func govueIOSGameLoad(_ handle: UnsafeMutableRawPointer, _ data: UnsafePointer<UInt8>, _ length: Int) {
    if length > 0, length <= 1_000_000, let text = String(bytes: UnsafeBufferPointer(start: data, count: length), encoding: .utf8) {
        Unmanaged<GameRenderer>.fromOpaque(handle).takeUnretainedValue().load(text)
    }
}

@_cdecl("govue_ios_game_reply")
func govueIOSGameReply(_ handle: UnsafeMutableRawPointer, _ data: UnsafePointer<UInt8>, _ length: Int) {
    if length > 0, length <= 100_000, let text = String(bytes: UnsafeBufferPointer(start: data, count: length), encoding: .utf8) {
        Unmanaged<GameRenderer>.fromOpaque(handle).takeUnretainedValue().reply(text)
    }
}

@_cdecl("govue_ios_game_close")
func govueIOSGameClose(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<GameRenderer>.fromOpaque(handle).takeRetainedValue().close()
}
