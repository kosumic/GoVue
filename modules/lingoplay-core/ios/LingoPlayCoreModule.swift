import ExpoModulesCore
import Foundation

public final class LingoPlayCoreModule: Module {
  public func definition() -> ModuleDefinition {
    Name("LingoPlayCore")
    AsyncFunction("execute") { (request: String) -> String in
      let directory = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        .appendingPathComponent("LingoPlay/transcripts", isDirectory: true)
      return execute(request: request, storagePath: directory.path)
    }
  }
}
