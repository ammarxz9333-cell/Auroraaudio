from pathlib import Path

p = Path("DAViewer/lib/core/runtime/app_runtime.dart")
s = p.read_text()

old_bridge = """    final webViewOAuthBridge = WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final oauth = DAKitOAuthClient(
"""
new_bridge = """    final useSystemBrowserOAuth = Platform.isAndroid;
    final webViewOAuthBridge =
        useSystemBrowserOAuth ? null : WebViewOAuthBridge();
    final webViewProxyManager = proxyController == null
        ? null
        : WebViewProxyManager(proxyController);
    final oauth = DAKitOAuthClient(
"""
if old_bridge not in s:
    raise SystemExit("runtime bridge anchor changed")
s = s.replace(old_bridge, new_bridge, 1)

old_auth = """      launcher: webViewOAuthBridge,
      callbacks: MergedCallbackUriSource(
        initial: AppLinksCallbackUriSource(),
        others: <CallbackUriSource>[webViewOAuthBridge.callbacks],
      ),
"""
new_auth = """      launcher: useSystemBrowserOAuth
          ? const SystemUriLauncher()
          : webViewOAuthBridge!,
      callbacks: useSystemBrowserOAuth
          ? AppLinksCallbackUriSource()
          : MergedCallbackUriSource(
              initial: AppLinksCallbackUriSource(),
              others: <CallbackUriSource>[webViewOAuthBridge!.callbacks],
            ),
"""
if old_auth not in s:
    raise SystemExit("runtime OAuth anchor changed")
s = s.replace(old_auth, new_auth, 1)
p.write_text(s)
