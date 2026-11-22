import Foundation
import UserNotifications

typealias ReplyNotificationOpened = @convention(c) (UnsafeRawPointer, UnsafePointer<CChar>) -> Void

private final class ReplyNotifications: NSObject, UNUserNotificationCenterDelegate, @unchecked Sendable {
    static let shared = ReplyNotifications()
    private var revisions: [String: UInt64] = [:]
    private var pending: String?
    private var observer: (context: UnsafeRawPointer, open: ReplyNotificationOpened)?

    func prepare() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in }
    }

    func update(_ chat: String, show: Bool, revision: UInt64) {
        DispatchQueue.main.async { [self] in
            guard revision > revisions[chat, default: 0] else { return }
            let previous = revisions.updateValue(revision, forKey: chat)
            let center = UNUserNotificationCenter.current()
            let prefix = "govue.chat." + chat + "."
            let identifier = prefix + String(revision)
            if let previous {
                let old = prefix + String(previous)
                center.removePendingNotificationRequests(withIdentifiers: [old])
                center.removeDeliveredNotifications(withIdentifiers: [old])
            }
            if !show {
                // Include alerts delivered by an earlier launch of GoVue.
                center.getDeliveredNotifications { alerts in
                    DispatchQueue.main.async { [self] in
                        guard revisions[chat] == revision else { return }
                        center.removeDeliveredNotifications(withIdentifiers: alerts.map(\.request.identifier).filter { $0.hasPrefix(prefix) })
                    }
                }
                center.getPendingNotificationRequests { requests in
                    DispatchQueue.main.async { [self] in
                        guard revisions[chat] == revision else { return }
                        center.removePendingNotificationRequests(withIdentifiers: requests.map(\.identifier).filter { $0.hasPrefix(prefix) })
                    }
                }
                return
            }
            let content = UNMutableNotificationContent()
            content.title = "Chat reply ready"
            content.body = "Tap to open your conversation."
            content.sound = .default
            content.userInfo = ["chatId": chat, "revision": revision]
            center.add(UNNotificationRequest(identifier: identifier, content: content, trigger: nil)) { _ in
                DispatchQueue.main.async { [self] in
                    if revisions[chat] != revision {
                        center.removePendingNotificationRequests(withIdentifiers: [identifier])
                        center.removeDeliveredNotifications(withIdentifiers: [identifier])
                    }
                }
            }
        }
    }

    func observe(_ context: UnsafeRawPointer, open: @escaping ReplyNotificationOpened) {
        observer = (context, open)
        // Wait for the caller to finish registering the Native Shell view.
        DispatchQueue.main.async { [self] in drain() }
    }

    func remove(_ context: UnsafeRawPointer) {
        if observer?.context == context { observer = nil }
    }

    private func drain() {
        guard let pending, let observer else { return }
        self.pending = nil
        pending.withCString { observer.open(observer.context, $0) }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, willPresent notification: UNNotification,
        withCompletionHandler completion: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        DispatchQueue.main.async { [self] in
            let data = notification.request.content.userInfo
            guard let chat = data["chatId"] as? String, let revision = data["revision"] as? NSNumber,
                  revisions[chat] == revision.uint64Value else { completion([]); return }
            completion([.banner, .list, .sound])
        }
    }

    func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completion: @escaping () -> Void
    ) {
        defer { completion() }
        let request = response.notification.request
        guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
              request.identifier.hasPrefix("govue.chat."),
              let chat = request.content.userInfo["chatId"] as? String,
              chat.range(of: "^[A-Za-z0-9_-]{1,64}$", options: .regularExpression) != nil else { return }
        DispatchQueue.main.async { [self] in
            pending = chat
            drain()
        }
    }
}

@_cdecl("govue_reply_notifications_install")
func govueReplyNotificationsInstall() {
    UNUserNotificationCenter.current().delegate = ReplyNotifications.shared
}

@_cdecl("govue_reply_notifications_update")
func govueReplyNotificationsUpdate(_ chat: UnsafePointer<CChar>, _ show: Bool, _ revision: UInt64) {
    ReplyNotifications.shared.update(String(cString: chat), show: show, revision: revision)
}

@_cdecl("govue_reply_notifications_prepare")
func govueReplyNotificationsPrepare() {
    ReplyNotifications.shared.prepare()
}

@_cdecl("govue_reply_notifications_observe")
func govueReplyNotificationsObserve(_ context: UnsafeRawPointer, _ open: @escaping ReplyNotificationOpened) {
    ReplyNotifications.shared.observe(context, open: open)
}

@_cdecl("govue_reply_notifications_remove")
func govueReplyNotificationsRemove(_ context: UnsafeRawPointer) {
    ReplyNotifications.shared.remove(context)
}
