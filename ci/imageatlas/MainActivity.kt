package com.imageatlas.imageatlas

import android.content.ContentValues
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.FileOutputStream

class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "imageatlas/downloads")
            .setMethodCallHandler { call, result ->
                if (call.method != "saveImage") {
                    result.notImplemented()
                    return@setMethodCallHandler
                }
                val bytes = call.argument<ByteArray>("bytes")
                val requestedName = call.argument<String>("filename")
                val mime = call.argument<String>("mime") ?: "image/jpeg"
                if (bytes == null || bytes.isEmpty()) {
                    result.error("EMPTY", "No image bytes were provided.", null)
                    return@setMethodCallHandler
                }
                try {
                    val safeName = sanitizeFilename(
                        requestedName ?: "image_${System.currentTimeMillis()}.jpg"
                    )
                    result.success(saveImage(bytes, safeName, mime))
                } catch (error: Throwable) {
                    result.error("SAVE_FAILED", error.message, null)
                }
            }
    }

    private fun saveImage(bytes: ByteArray, filename: String, mime: String): String {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.Images.Media.DISPLAY_NAME, filename)
                put(MediaStore.Images.Media.MIME_TYPE, mime)
                put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/ImageAtlas")
                put(MediaStore.Images.Media.IS_PENDING, 1)
            }
            val uri = contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
                ?: error("Could not create MediaStore item.")
            contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                ?: error("Could not open image output stream.")
            values.clear()
            values.put(MediaStore.Images.Media.IS_PENDING, 0)
            contentResolver.update(uri, values, null, null)
            return uri.toString()
        }

        @Suppress("DEPRECATION")
        val dir = File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_PICTURES),
            "ImageAtlas"
        )
        if (!dir.exists() && !dir.mkdirs()) error("Could not create ImageAtlas folder.")
        val file = File(dir, filename)
        FileOutputStream(file).use { it.write(bytes) }
        return file.absolutePath
    }

    private fun sanitizeFilename(raw: String): String {
        val cleaned = raw.replace(Regex("[^A-Za-z0-9._-]"), "_").take(120)
        return cleaned.ifBlank { "image_${System.currentTimeMillis()}.jpg" }
    }
}
