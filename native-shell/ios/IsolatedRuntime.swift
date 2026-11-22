import UIKit
import WebKit

typealias RuntimeCallback = @convention(c) (
  UnsafeRawPointer, UnsafePointer<CChar>, UnsafeRawPointer, Int
) -> Void

@MainActor
private enum NetworkRules {
  private static var rule: Task<WKContentRuleList?, Never>?

  static func load() async -> WKContentRuleList? {
    if let rule { return await rule.value }
    let pending = Task {
      let entries = ["http", "https", "ws", "wss", "ftp", "file"].map { scheme in
        ["trigger": ["url-filter": "^" + scheme + ":"], "action": ["type": "block"]]
      }
      guard let json = try? JSONSerialization.data(withJSONObject: entries),
            let source = String(data: json, encoding: .utf8) else { return nil as WKContentRuleList? }
      return try? await WKContentRuleListStore.default().compileContentRuleList(
        forIdentifier: "GoVue.isolated-renderer.v1",
        encodedContentRuleList: source
      )
    }
    rule = pending
    return await pending.value
  }
}

@MainActor
private final class LocalPage: NSObject, WKNavigationDelegate {
  static let header = """
    <!doctype html><meta charset="utf-8">
    <meta http-equiv="Content-Security-Policy"
      content="default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; connect-src 'none';
        base-uri 'none'; form-action 'none'; frame-src 'none'; worker-src 'none'">
    <script>
    for (const name of ['RTCPeerConnection', 'webkitRTCPeerConnection', 'WebTransport'])
      Object.defineProperty(window, name, { value: undefined, configurable: false, writable: false });
    Object.defineProperty(Document.prototype, 'cookie', { get: () => '', set: () => {} });
    </script>
    """

  private weak var container: UIView?
  private var web: WKWebView?
  private var load: Task<Void, Never>?
  private var initial: WKNavigation?
  private var allowInitial = true
  private var loaded: ((WKWebView) -> Void)?
  private var failed: (() -> Void)?

  init(container: UIView) { self.container = container }

  func open(_ html: String, loaded: @escaping (WKWebView) -> Void, failed: @escaping () -> Void) {
    self.loaded = loaded
    self.failed = failed
    load = Task { [weak self] in
      let rules = await NetworkRules.load()
      guard !Task.isCancelled, let self else { return }
      guard let rules, let container = self.container else { self.fail(); return }
      let configuration = WKWebViewConfiguration()
      configuration.websiteDataStore = .nonPersistent()
      configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
      configuration.userContentController.add(rules)
      let view = WKWebView(frame: CGRect(x: 0, y: 0, width: 1, height: 1), configuration: configuration)
      view.navigationDelegate = self
      view.isUserInteractionEnabled = false
      container.addSubview(view)
      self.web = view
      self.initial = view.loadHTMLString(html, baseURL: nil)
      if self.initial == nil { self.fail() }
    }
  }

  func webView(_ view: WKWebView, decidePolicyFor action: WKNavigationAction,
               decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
    let allowed = allowInitial && action.targetFrame?.isMainFrame == true
      && action.request.url?.absoluteString == "about:blank"
    if allowed { allowInitial = false }
    decisionHandler(allowed ? .allow : .cancel)
  }

  func webView(_ view: WKWebView, didFinish navigation: WKNavigation!) {
    guard web === view, navigation === initial, view.url?.absoluteString == "about:blank" else { return }
    let callback = loaded
    loaded = nil
    initial = nil
    callback?(view)
  }

  func webView(_ view: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
    if navigation === initial { fail() }
  }

  func webView(_ view: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
    if navigation === initial { fail() }
  }

  func webViewWebContentProcessDidTerminate(_ view: WKWebView) { fail() }

  func browser() -> WKWebView? { initial == nil ? web : nil }

  private func fail() {
    let callback = failed
    close()
    callback?()
  }

  func close() {
    load?.cancel()
    load = nil
    loaded = nil
    failed = nil
    initial = nil
    web?.navigationDelegate = nil
    web?.stopLoading()
    web?.removeFromSuperview()
    web = nil
  }
}

@MainActor
private final class Runtime {
  private static let tokenOutput = """
    (() => {
      let value = null;
      Object.defineProperty(window, '__govueTokenTransport', { value: Object.freeze({
        postMessage: raw => { if (typeof raw === 'string' && raw.length <= 2100000) value = raw; }
      }) });
      Object.defineProperty(window, '__govueTakeToken', { value: () => {
        const raw = value; value = null; return raw;
      } });
    })();
    """

  private final class TokenJob {
    let page: LocalPage
    var task: UInt64?
    var minting = false
    init(_ page: LocalPage) { self.page = page }
  }

  private let context: UnsafeRawPointer
  private let reply: RuntimeCallback
  private let tokenMode: Bool
  private weak var view: UIView?
  private var tokenDocument: String?
  private var jobs: [String: TokenJob] = [:]
  private var signer: (request: String, task: String, page: LocalPage)?

  init(view: UIView, context: UnsafeRawPointer, tokenMode: Bool, reply: RuntimeCallback) {
    self.view = view
    self.context = context
    self.tokenMode = tokenMode
    self.reply = reply
  }

  func script(_ source: String) {
    guard tokenMode, tokenDocument == nil, source.utf8.count <= 2_000_000 else { return }
    let escaped = source.replacingOccurrences(of: "</script", with: #"<\/script"#, options: .caseInsensitive)
    tokenDocument = LocalPage.header + "<script>" + Self.tokenOutput + escaped + "</script>"
    emit("ready", NSNull())
  }

  func command(_ name: String, args: Any) {
    if name == "cancel", !tokenMode, let task = args as? String, signer?.task == task {
      signer?.page.close()
      signer = nil
    } else if name == "release", tokenMode, let request = args as? String {
      jobs.removeValue(forKey: request)?.page.close()
    } else if name == "evaluate", let fields = args as? [String: Any],
              let request = fields["requestId"] as? String, (1...100).contains(request.utf8.count) {
      if tokenMode {
        evaluateToken(request, fields: fields)
      } else {
        evaluateSignature(request, fields: fields)
      }
    }
  }

  private func evaluateSignature(_ request: String, fields: [String: Any]) {
    guard let task = fields["taskId"] as? String, (1...20).contains(task.utf8.count) else { return }
    guard signer == nil, let view,
          let source = fields["source"] as? String, (1...2_000_000).contains(source.utf16.count) else {
      emit("evaluated", ["requestId": request, "taskId": task])
      return
    }
    let page = LocalPage(container: view)
    signer = (request, task, page)
    page.open(LocalPage.header, loaded: { [weak self] web in
      web.callAsyncJavaScript("""
        try {
          const result = Function(source)();
          const signatures = Object.create(null);
          for (const name of ['n', 'sig']) {
            const value = result?.[name];
            if (typeof value === 'string' && value.length <= 10000) signatures[name] = value;
          }
          return signatures;
        } catch (_) { return null; }
        """, arguments: ["source": source], in: nil, in: .page) { result in
        let output = try? result.get() as? [String: Any]
        self?.signature(request, task: task, output: output)
      }
    }, failed: { [weak self] in self?.signature(request, task: task, output: nil) })
  }

  private func signature(_ request: String, task: String, output: [String: Any]?) {
    guard let active = signer, active.request == request, active.task == task else { return }
    active.page.close()
    signer = nil
    var reply: [String: Any] = ["requestId": request, "taskId": task]
    if let output {
      reply["signatures"] = output.filter { name, value in
        (name == "n" || name == "sig") && ((value as? String)?.utf16.count ?? 10001) <= 10000
      }
    }
    emit("evaluated", reply)
  }

  private func evaluateToken(_ request: String, fields: [String: Any]) {
    guard let input = fields["task"] as? [String: Any],
          let task = input["taskId"] as? UInt64, task > 0,
          let stage = input["stage"] as? String, let tokenDocument else { return }
    if stage == "snapshot", jobs[request] == nil, jobs.count < 4, let view {
      let page = LocalPage(container: view)
      let job = TokenJob(page)
      job.task = task
      jobs[request] = job
      page.open(tokenDocument, loaded: { [weak self] web in
        self?.runToken(request, task: task, input: input, web: web)
      }, failed: { [weak self] in self?.finishToken(request, task: task, raw: nil) })
    } else if stage == "mint", let job = jobs[request], job.task == nil, let web = job.page.browser() {
      job.minting = true
      job.task = task
      runToken(request, task: task, input: input, web: web)
    } else {
      emit("token", ["requestId": request, "taskId": task, "value": NSNull()])
    }
  }

  private func runToken(_ request: String, task: UInt64, input: [String: Any], web: WKWebView) {
    guard jobs[request]?.task == task else { return }
    web.callAsyncJavaScript("""
      window.__govueTakeToken?.();
      await window.runExtractionToken?.(request, input);
      return window.__govueTakeToken?.();
      """, arguments: ["request": request, "input": input], in: nil, in: .page) { [weak self] result in
      self?.finishToken(request, task: task, raw: try? result.get() as? String)
    }
  }

  private func finishToken(_ request: String, task: UInt64, raw: String?) {
    guard let job = jobs[request], job.task == task else { return }
    var value: String?
    if let raw, raw.utf8.count <= 2_001_000,
       let object = try? JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any],
       object["requestId"] as? String == request, object["taskId"] as? UInt64 == task,
       object["error"] as? Bool != true, let token = object["value"] as? String,
       (1...2_000_000).contains(token.utf8.count) {
      value = token
    }
    job.task = nil
    if value == nil || job.minting { jobs.removeValue(forKey: request)?.page.close() }
    emit("token", ["requestId": request, "taskId": task, "value": value as Any? ?? NSNull()])
  }

  private func emit(_ event: String, _ payload: Any) {
    guard let data = try? JSONSerialization.data(withJSONObject: payload, options: .fragmentsAllowed) else { return }
    event.withCString { name in
      data.withUnsafeBytes { bytes in
        if let address = bytes.baseAddress { reply(context, name, address, bytes.count) }
      }
    }
  }

  func stop() {
    for job in jobs.values { job.page.close() }
    jobs.removeAll()
    signer?.page.close()
    signer = nil
  }
}

@_cdecl("govue_renderer_create")
@MainActor
func createRenderer(view: UnsafeRawPointer, context: UnsafeRawPointer, tokens: Bool, callback: RuntimeCallback)
    -> UnsafeMutableRawPointer {
  let view = Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue()
  return Unmanaged.passRetained(Runtime(view: view, context: context, tokenMode: tokens, reply: callback)).toOpaque()
}

@_cdecl("govue_renderer_send")
@MainActor
func sendToRenderer(handle: UnsafeMutableRawPointer, method: UnsafePointer<CChar>, bytes: UnsafeRawPointer, count: Int) {
  guard count > 0, count <= 8_100_000,
        let args = try? JSONSerialization.jsonObject(with: Data(bytes: bytes, count: count), options: .fragmentsAllowed)
  else { return }
  let runtime = Unmanaged<Runtime>.fromOpaque(handle).takeUnretainedValue()
  let method = String(cString: method)
  if method == "script", let source = args as? String { runtime.script(source) }
  else { runtime.command(method, args: args) }
}

@_cdecl("govue_renderer_close")
@MainActor
func closeRenderer(handle: UnsafeMutableRawPointer) {
  Unmanaged<Runtime>.fromOpaque(handle).takeRetainedValue().stop()
}
