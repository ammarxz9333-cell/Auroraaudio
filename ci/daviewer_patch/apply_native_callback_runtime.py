from pathlib import Path

p = Path("DAViewer/lib/core/runtime/app_runtime.dart")
s = p.read_text()

s = s.replace(
    "import '../auth/webview_oauth_bridge.dart';\n",
    "import '../auth/android_native_oauth_callback_source.dart';\nimport '../auth/webview_oauth_bridge.dart';\n",
    1,
)

s = s.replace(
"""    this.webViewOAuthBridge,
    this.webViewProxyManager,
  });""",
"""    this.webViewOAuthBridge,
    this.webViewProxyManager,
    this.androidNativeOAuthCallbackSource,
  });""",
    1,
)

s = s.replace(
"""  final WebViewOAuthBridge? webViewOAuthBridge;
  final WebViewProxyManager? webViewProxyManager;

  void Function()? _proxyListener;""",
"""  final WebViewOAuthBridge? webViewOAuthBridge;
  final WebViewProxyManager? webViewProxyManager;
  final AndroidNativeOAuthCallbackSource? androidNativeOAuthCallbackSource;

  void Function()? _proxyListener;""",
    1,
)

anchor = """    final webViewOAuthBridge = WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final oauth = DAKitOAuthClient(
"""
replacement = """    final webViewOAuthBridge = WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final appLinksCallbackSource = AppLinksCallbackUriSource();
    final androidNativeOAuthCallbackSource = Platform.isAndroid
        ? AndroidNativeOAuthCallbackSource()
        : null;
    final oauth = DAKitOAuthClient(
"""
if anchor not in s:
    raise SystemExit("runtime bridge anchor changed")
s = s.replace(anchor, replacement, 1)

old_callbacks = """      callbacks: MergedCallbackUriSource(
        initial: AppLinksCallbackUriSource(),
        others: <CallbackUriSource>[webViewOAuthBridge.callbacks],
      ),"""
new_callbacks = """      callbacks: Platform.isAndroid
          ? MergedCallbackUriSource(
              initial: androidNativeOAuthCallbackSource!,
              others: <CallbackUriSource>[
                appLinksCallbackSource,
                webViewOAuthBridge.callbacks,
              ],
            )
          : MergedCallbackUriSource(
              initial: appLinksCallbackSource,
              others: <CallbackUriSource>[webViewOAuthBridge.callbacks],
            ),"""
if old_callbacks not in s:
    raise SystemExit("runtime callback anchor changed")
s = s.replace(old_callbacks, new_callbacks, 1)

s = s.replace(
"""      webViewOAuthBridge: webViewOAuthBridge,
      webViewProxyManager: webViewProxyManager,
    );""",
"""      webViewOAuthBridge: webViewOAuthBridge,
      webViewProxyManager: webViewProxyManager,
      androidNativeOAuthCallbackSource: androidNativeOAuthCallbackSource,
    );""",
    1,
)

s = s.replace(
"""    unawaited(webViewOAuthBridge?.dispose() ?? Future<void>.value());
  }""",
"""    unawaited(webViewOAuthBridge?.dispose() ?? Future<void>.value());
    unawaited(
      androidNativeOAuthCallbackSource?.dispose() ?? Future<void>.value(),
    );
  }""",
    1,
)

p.write_text(s)
