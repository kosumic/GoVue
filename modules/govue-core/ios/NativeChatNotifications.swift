import ExpoModulesCore
import UIKit
import UserNotifications

// Installed before React starts so taps also survive a cold launch.
public final class ChatNotificationAppDelegate: ExpoAppDelegateSubscriber {
  public func application(_ application: UIApplication, didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil) -> Bool {
    UNUserNotificationCenter.current().delegate = NativeChatNotifications.shared
    return true
  }
  public func applicationDidBecomeActive(_ application: UIApplication) {
    _ = stateExecute(request: "{\"op\":\"chat_notification_foreground\",\"active\":true}")
  }
  public func applicationDidEnterBackground(_ application: UIApplication) {
    _ = stateExecute(request: "{\"op\":\"chat_notification_foreground\",\"active\":false}")
  }
}

// Presentation revisions are confined to main; launch handoff uses launchLock.
final class NativeChatNotifications: NSObject, ChatNotificationPlatform, UNUserNotificationCenterDelegate, @unchecked Sendable {
  static let shared = NativeChatNotifications()
  static let opened = Notification.Name("GoVueChatNotificationOpened")
  private var revisions: [String: UInt64] = [:]
  private let launchLock = NSLock()
  private var launch: String?

  func takeLaunches() -> [String] {
    launchLock.lock(); defer { launchLock.unlock() }
    defer { launch = nil }
    return launch.map { [$0] } ?? []
  }

  func prepare(_ promise: Promise) {
    UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in
      promise.resolve(nil)
    }
  }

  func update(chatId: String, show: Bool, revision: UInt64) {
    DispatchQueue.main.async { [self] in
      guard revision > revisions[chatId, default: 0] else { return }
      let previous = revisions[chatId]
      revisions[chatId] = revision
      let center = UNUserNotificationCenter.current()
      let prefix = "govue.chat." + chatId + "."
      let identifier = prefix + String(revision)
      if let previous {
        let old = prefix + String(previous)
        center.removePendingNotificationRequests(withIdentifiers: [old])
        center.removeDeliveredNotifications(withIdentifiers: [old])
      }
      if !show {
        // Also clear alerts delivered by an earlier process, whose revision
        // counter is no longer in memory. A newer presentation wins this race.
        center.getDeliveredNotifications { alerts in
          DispatchQueue.main.async { [self] in
            guard revisions[chatId] == revision else { return }
            center.removeDeliveredNotifications(withIdentifiers: alerts.map(\.request.identifier).filter { $0.hasPrefix(prefix) })
          }
        }
        center.getPendingNotificationRequests { requests in
          DispatchQueue.main.async { [self] in
            guard revisions[chatId] == revision else { return }
            center.removePendingNotificationRequests(withIdentifiers: requests.map(\.identifier).filter { $0.hasPrefix(prefix) })
          }
        }
        return
      }
      let content = UNMutableNotificationContent()
      content.title = "Chat reply ready"
      content.body = "Tap to open your conversation."
      content.sound = .default
      content.userInfo = ["chatId": chatId, "revision": revision]
      let request = UNNotificationRequest(identifier: identifier, content: content, trigger: nil)
      center.add(request) { _ in
        DispatchQueue.main.async { [self] in
          if revisions[chatId] != revision {
            center.removePendingNotificationRequests(withIdentifiers: [identifier])
            center.removeDeliveredNotifications(withIdentifiers: [identifier])
          }
        }
      }
    }
  }

  func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
    DispatchQueue.main.async { [self] in
      let data = notification.request.content.userInfo
      guard let chat = data["chatId"] as? String,
        let revision = data["revision"] as? NSNumber,
        revisions[chat] == revision.uint64Value else { completionHandler([]); return }
      completionHandler([.banner, .list, .sound])
    }
  }

  func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
    defer { completionHandler() }
    guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
      response.notification.request.identifier.hasPrefix("govue.chat."),
      let chat = response.notification.request.content.userInfo["chatId"] as? String,
      chat.range(of: "^[A-Za-z0-9_-]{1,64}$", options: .regularExpression) != nil else { return }
    launchLock.lock(); launch = chat; launchLock.unlock()
    NotificationCenter.default.post(name: Self.opened, object: nil)
  }
}
