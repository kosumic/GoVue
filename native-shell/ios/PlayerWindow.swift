import UIKit

typealias AttachNativeShell = @convention(c) (UnsafeRawPointer) -> Void
typealias OpenNativeShellURL = @convention(c) (UnsafePointer<CChar>) -> Void
typealias StatusBarValue = @convention(c) () -> Int
typealias StatusBarHidden = @convention(c) () -> Bool

private struct HostCallbacks {
    let attach: AttachNativeShell
    let open: OpenNativeShellURL
    let style: StatusBarValue
    let hidden: StatusBarHidden
    let animation: StatusBarValue
}

private var host: HostCallbacks?

@objc(GoVueAppDelegate)
final class GoVueAppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: nil, sessionRole: session.role)
        configuration.delegateClass = GoVueSceneDelegate.self
        return configuration
    }
}

@objc(GoVueSceneDelegate)
final class GoVueSceneDelegate: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?

    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options connectionOptions: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        let controller = GoVuePlayerController()
        controller.view.backgroundColor = .systemBackground
        window.rootViewController = controller
        self.window = window
        window.makeKeyAndVisible()
        host?.attach(Unmanaged.passUnretained(controller.view).toOpaque())
        for context in connectionOptions.urlContexts { open(context.url) }
        for activity in connectionOptions.userActivities { open(activity.webpageURL) }
    }

    func scene(_ scene: UIScene, openURLContexts contexts: Set<UIOpenURLContext>) {
        for context in contexts { open(context.url) }
    }

    func scene(_ scene: UIScene, continue activity: NSUserActivity) {
        open(activity.webpageURL)
    }

    private func open(_ url: URL?) {
        url?.absoluteString.withCString { host?.open($0) }
    }
}

private final class GoVuePlayerController: UIViewController {
    private var fullscreen = false
    private var previous: UIInterfaceOrientationMask = .portrait

    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        fullscreen ? .landscape : super.supportedInterfaceOrientations
    }

    override var prefersStatusBarHidden: Bool { fullscreen || host?.hidden() == true }
    override var preferredStatusBarStyle: UIStatusBarStyle {
        host.flatMap { UIStatusBarStyle(rawValue: $0.style()) } ?? .default
    }
    override var preferredStatusBarUpdateAnimation: UIStatusBarAnimation {
        host.flatMap { UIStatusBarAnimation(rawValue: $0.animation()) } ?? .none
    }

    func setFullscreen(_ enabled: Bool) {
        guard enabled != fullscreen, let scene = view.window?.windowScene else { return }
        if enabled {
            let orientation: UIInterfaceOrientation
            if #available(iOS 26.0, *) { orientation = scene.effectiveGeometry.interfaceOrientation }
            else { orientation = scene.interfaceOrientation }
            previous = orientation == .unknown ? .portrait : UIInterfaceOrientationMask(rawValue: 1 << orientation.rawValue)
        }
        fullscreen = enabled
        setNeedsStatusBarAppearanceUpdate()
        setNeedsUpdateOfSupportedInterfaceOrientations()
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: enabled ? .landscape : previous)) { error in
            NSLog("GoVue could not change window orientation: %@", error.localizedDescription)
        }
    }
}

@_cdecl("govue_ios_run")
func govueIOSRun(_ attach: @escaping AttachNativeShell, _ open: @escaping OpenNativeShellURL,
                 _ style: @escaping StatusBarValue, _ hidden: @escaping StatusBarHidden,
                 _ animation: @escaping StatusBarValue) -> Int32 {
    host = HostCallbacks(attach: attach, open: open, style: style, hidden: hidden, animation: animation)
    return UIApplicationMain(CommandLine.argc, CommandLine.unsafeArgv, nil, NSStringFromClass(GoVueAppDelegate.self))
}

@_cdecl("govue_ios_player_fullscreen")
func govueIOSPlayerFullscreen(_ view: UnsafeRawPointer, _ enabled: Bool) {
    let view = Unmanaged<UIView>.fromOpaque(view).takeUnretainedValue()
    (view.window?.rootViewController as? GoVuePlayerController)?.setFullscreen(enabled)
}
