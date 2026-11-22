package io.github.kaminarios.govue.shell

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.annotation.OptIn
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ExperimentalGetImage
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import com.google.mlkit.vision.barcode.BarcodeScannerOptions
import com.google.mlkit.vision.barcode.BarcodeScanning
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.common.InputImage
import com.longbridge.nsh.NshComponent
import com.longbridge.nsh.NshComponentContext
import com.longbridge.nsh.NshView
import java.io.File

class ProviderQrScanner private constructor(
  private val activity: Activity,
  private val events: NshComponentContext,
) : NshComponent, LifecycleOwner {
  private val preview = PreviewView(activity).apply { implementationMode = PreviewView.ImplementationMode.COMPATIBLE }
  private val registry = LifecycleRegistry(this).apply { currentState = Lifecycle.State.CREATED }
  override val lifecycle: Lifecycle get() = registry
  private val main = ContextCompat.getMainExecutor(activity)
  private val detector = BarcodeScanning.getClient(
    BarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).build()
  )
  private var provider: ProcessCameraProvider? = null
  private var useCases = emptyList<androidx.camera.core.UseCase>()
  private var camera: Camera? = null
  private var enabled = false
  private var torch = false
  private var asked = false
  private var closed = false
  private var generation = 0
  private var handled = false
  private var imageRequest: String? = null

  override fun view() = preview

  override fun setProp(name: String, value: Any?) {
    when (name) {
      "camera" -> {
        val next = value == true
        if (enabled == next) return
        enabled = next
        if (next) {
          if (permission()) start()
          else if (!asked) request()
        } else {
          stop()
        }
      }
      "torch" -> {
        torch = value == true
        camera?.cameraControl?.enableTorch(torch)
      }
      "readImage" -> if (value is Map<*, *>) decodeImage(value)
    }
  }

  override fun command(name: String, args: Any?) {
    when (name) {
      "permission" -> {
        if (permission()) return
        if (!asked || activity.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)) {
          request()
        } else {
          val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", activity.packageName, null))
          runCatching { activity.startActivity(intent) }
            .onFailure { error("Could not open camera permissions.") }
        }
      }
    }
  }

  fun resume() {
    if (enabled && camera == null && permission()) start()
  }

  fun permissionChanged() {
    if (enabled && permission()) start()
  }

  private fun request() {
    asked = true
    activity.requestPermissions(arrayOf(Manifest.permission.CAMERA), REQUEST)
  }

  private fun permission(): Boolean {
    val granted = activity.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
    events.emit("permission", mapOf(
      "granted" to granted,
      "canAskAgain" to (!asked || activity.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)),
    ))
    return granted
  }

  private fun start() {
    if (closed || camera != null) return
    handled = false
    val request = ++generation
    val future = ProcessCameraProvider.getInstance(activity)
    future.addListener({
      if (closed || !enabled || request != generation) return@addListener
      runCatching {
        val host = future.get()
        provider = host
        val video = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
        val images = ImageAnalysis.Builder()
          .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
        images.setAnalyzer(main) { frame -> analyze(frame, request) }
        registry.currentState = Lifecycle.State.STARTED
        camera = host.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, video, images)
        useCases = listOf(video, images)
        camera?.cameraControl?.enableTorch(torch)
      }.onFailure {
        stop()
        error("Could not start the camera. Close the scanner and try again.")
      }
    }, main)
  }

  @OptIn(markerClass = [ExperimentalGetImage::class])
  private fun analyze(frame: ImageProxy, revision: Int) {
    val image = frame.image
    if (image == null || closed || handled || !enabled || revision != generation) {
      frame.close()
      return
    }
    val input = InputImage.fromMediaImage(image, frame.imageInfo.rotationDegrees)
    detector.process(input).addOnCompleteListener(main) { task ->
      frame.close()
      if (closed || handled || !enabled || revision != generation || !task.isSuccessful) return@addOnCompleteListener
      val values = payloads(task.result)
      if (values.isNotEmpty()) {
        handled = true
        events.emit("codes", mapOf("values" to values))
      }
    }
  }

  private fun decodeImage(args: Map<*, *>) {
    val request = args["request"] as? String ?: return
    if (request == imageRequest) return
    imageRequest = request
    val path = args["path"] as? String ?: return
    val file = runCatching { File(path).canonicalFile }.getOrNull()
    val sandbox = File(activity.applicationInfo.dataDir).canonicalFile.path + File.separator
    if (file == null || !file.path.startsWith(sandbox) || !file.isFile || file.length() !in 1..20_000_000) {
      imageResult(request, emptyList(), "Could not read the selected QR image.")
      return
    }
    val image = runCatching { InputImage.fromFilePath(activity, Uri.fromFile(file)) }.getOrNull()
    if (image == null) {
      imageResult(request, emptyList(), "Could not read the selected QR image.")
      return
    }
    detector.process(image).addOnCompleteListener(main) { result ->
      if (closed) return@addOnCompleteListener
      if (result.isSuccessful) imageResult(request, payloads(result.result), null)
      else imageResult(request, emptyList(), "Could not read the selected QR image.")
    }
  }

  private fun payloads(codes: List<Barcode>) = codes.mapNotNull { it.rawValue?.takeIf { value -> value.length <= 1000 } }.take(10)

  private fun imageResult(request: String, values: List<String>, error: String?) {
    val result = if (error == null) mapOf("request" to request, "values" to values)
      else mapOf("request" to request, "error" to error)
    events.emit("codes", result)
  }

  private fun error(message: String) = events.emit("error", message)

  private fun stop() {
    generation++
    registry.currentState = Lifecycle.State.CREATED
    camera?.cameraControl?.enableTorch(false)
    camera = null
    if (useCases.isNotEmpty()) provider?.unbind(*useCases.toTypedArray())
    useCases = emptyList()
  }

  override fun destroy() {
    closed = true
    stop()
    registry.currentState = Lifecycle.State.DESTROYED
    detector.close()
  }

  class Binding internal constructor() {
    private var scanner: ProviderQrScanner? = null

    internal fun register(view: NshView, activity: Activity) {
      view.registerComponent("govue/provider-qr-scanner") { context ->
        ProviderQrScanner(activity, context).let { fresh ->
          scanner = fresh
          object : NshComponent by fresh {
            override fun destroy() {
              if (scanner === fresh) scanner = null
              fresh.destroy()
            }
          }
        }
      }
    }

    fun resume() { scanner?.resume() }
    fun permissionChanged() { scanner?.permissionChanged() }
  }

  companion object {
    const val REQUEST = 9735

    @JvmStatic fun register(view: NshView, activity: Activity) =
      Binding().also { it.register(view, activity) }
  }
}
