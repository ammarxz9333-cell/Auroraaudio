package app.daviewer

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Uri
import android.os.Build
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private var oauthCallbackChannel: MethodChannel? = null
    private var pendingOAuthCallback: String? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        pendingOAuthCallback =
            pendingOAuthCallback ?: validatedOAuthCallback(intent?.dataString)

        oauthCallbackChannel = MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            "daviewer/oauth_callback",
        ).also { channel ->
            channel.setMethodCallHandler { call, result ->
                when (call.method) {
                    "getPendingOAuthCallback" -> {
                        val callback =
                            pendingOAuthCallback
                                ?: validatedOAuthCallback(intent?.dataString)
                        pendingOAuthCallback = null
                        result.success(callback)
                    }

                    "ackOAuthCallback" -> {
                        val callback = call.argument<String>("callback")
                        if (callback != null && pendingOAuthCallback == callback) {
                            pendingOAuthCallback = null
                        }
                        result.success(true)
                    }

                    else -> result.notImplemented()
                }
            }
        }

        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger,
            "daviewer/system_proxy",
        ).setMethodCallHandler { call, result ->
            if (call.method != "getSystemProxy") {
                result.notImplemented()
                return@setMethodCallHandler
            }
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) {
                result.success(null)
                return@setMethodCallHandler
            }
            val proxy = try {
                val connectivity = getSystemService(Context.CONNECTIVITY_SERVICE)
                    as ConnectivityManager
                connectivity.defaultProxy
            } catch (_: SecurityException) {
                null
            }
            val host = proxy?.host?.trim().orEmpty()
            val port = proxy?.port ?: -1
            if (host.isEmpty() || port !in 1..65535) {
                result.success(null)
            } else {
                result.success(mapOf("host" to host, "port" to port))
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        val callback = validatedOAuthCallback(intent.dataString) ?: return
        pendingOAuthCallback = callback

        val channel = oauthCallbackChannel ?: return
        channel.invokeMethod(
            "oauthCallback",
            callback,
            object : MethodChannel.Result {
                override fun success(result: Any?) {
                    // Dart returns true only when an OAuth stream listener was
                    // already attached and accepted this callback. Otherwise
                    // keep it pending so Dart can replay it when the listener
                    // subscribes a moment later.
                    if (result == true && pendingOAuthCallback == callback) {
                        pendingOAuthCallback = null
                    }
                }

                override fun error(
                    errorCode: String,
                    errorMessage: String?,
                    errorDetails: Any?,
                ) {
                    // Keep the callback pending for replay.
                }

                override fun notImplemented() {
                    // Dart is not ready yet. Keep it pending for replay.
                }
            },
        )
    }

    private fun validatedOAuthCallback(raw: String?): String? {
        if (raw.isNullOrBlank()) return null
        val uri = try {
            Uri.parse(raw)
        } catch (_: Throwable) {
            return null
        }
        if (!uri.scheme.equals("dakit", ignoreCase = true)) return null
        if (!uri.host.equals("oauth", ignoreCase = true)) return null
        if (uri.path != "/callback") return null
        return uri.toString()
    }
}
