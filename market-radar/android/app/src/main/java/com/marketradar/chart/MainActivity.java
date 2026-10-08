package com.marketradar.chart;
import android.app.Activity;
import android.os.Bundle;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.webkit.WebSettings;
import android.webkit.WebResourceRequest;
import android.content.Intent;
import android.net.Uri;
public final class MainActivity extends Activity {
 private WebView web;
 private static final String URL = "https://ammarxz9333-cell.github.io/Auroraaudio/";
 @Override public void onCreate(Bundle state) {
  super.onCreate(state);
  web = new WebView(this);
  web.getSettings().setJavaScriptEnabled(true);
  web.getSettings().setDomStorageEnabled(true);
  web.getSettings().setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
  web.setWebViewClient(new WebViewClient() {
   @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest req) {
    Uri u = req.getUrl();
    if ("https".equals(u.getScheme()) && "ammarxz9333-cell.github.io".equals(u.getHost()) && u.getPath().startsWith("/Auroraaudio/")) return false;
    try { startActivity(new Intent(Intent.ACTION_VIEW,u)); } catch(Exception ignored) {}
    return true;
   }
  });
  setContentView(web);
  web.loadUrl(URL);
 }
 @Override public void onBackPressed() { if(web.canGoBack()) web.goBack(); else super.onBackPressed(); }
 @Override public void onDestroy() { if(web!=null)web.destroy(); super.onDestroy(); }
}