package io.github.kaminarios.lingoplay.coremodule

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition
import io.github.kaminarios.lingoplay.core.execute
import java.io.File

class LingoPlayCoreModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("LingoPlayCore")
    AsyncFunction("execute") { request: String ->
      val context = requireNotNull(appContext.reactContext)
      execute(request, File(context.filesDir, "transcripts").path)
    }
  }
}
