package com.daviewer.v2;

import android.app.Activity;
import android.app.DownloadManager;
import android.content.Context;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.JavascriptInterface;
import android.webkit.URLUtil;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.Toast;

import java.util.LinkedHashMap;
import java.util.Map;

public class MainActivity extends Activity {
    private WebView webView;
    private ProgressBar progress;
    private Button downloadButton;
    private Button galleryButton;
    private final Map<String, String> sources = new LinkedHashMap<>();
    private String currentImageUrl = null;
    private String galleryUrl = null;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        sources.put("DeviantArt", "https://www.deviantart.com/");
        sources.put("ArtStation", "https://www.artstation.com/");
        sources.put("Behance", "https://www.behance.net/");
        sources.put("Pixiv", "https://www.pixiv.net/");
        sources.put("Flickr", "https://www.flickr.com/");

        getWindow().setStatusBarColor(Color.BLACK);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.BLACK);

        HorizontalScrollView sourceScroll = new HorizontalScrollView(this);
        sourceScroll.setHorizontalScrollBarEnabled(false);
        sourceScroll.setBackgroundColor(Color.rgb(18, 18, 18));

        LinearLayout sourceBar = new LinearLayout(this);
        sourceBar.setOrientation(LinearLayout.HORIZONTAL);
        sourceBar.setPadding(6, 6, 6, 6);

        for (Map.Entry<String, String> entry : sources.entrySet()) {
            Button b = button(entry.getKey());
            b.setOnClickListener(v -> openGallery(entry.getValue()));
            sourceBar.addView(b);
        }

        sourceScroll.addView(sourceBar);
        root.addView(sourceScroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));

        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 4));

        webView = new WebView(this);
        webView.setBackgroundColor(Color.BLACK);
        root.addView(webView, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));

        LinearLayout bottomBar = new LinearLayout(this);
        bottomBar.setOrientation(LinearLayout.HORIZONTAL);
        bottomBar.setPadding(8, 6, 8, 8);
        bottomBar.setBackgroundColor(Color.rgb(18, 18, 18));

        galleryButton = button("‹ Gallery");
        galleryButton.setEnabled(false);
        galleryButton.setOnClickListener(v -> returnToGallery());
        bottomBar.addView(galleryButton, new LinearLayout.LayoutParams(0, 56, 1f));

        downloadButton = button("⬇ Download");
        downloadButton.setEnabled(false);
        downloadButton.setOnClickListener(v -> {
            if (currentImageUrl != null) {
                download(currentImageUrl);
            } else {
                Toast.makeText(this, "Tap an image first", Toast.LENGTH_SHORT).show();
            }
        });
        bottomBar.addView(downloadButton, new LinearLayout.LayoutParams(0, 56, 1f));

        root.addView(bottomBar);
        setContentView(root);

        configureWebView();

        String last = getPreferences(MODE_PRIVATE)
                .getString("last_gallery", sources.get("DeviantArt"));
        openGallery(last);
    }

    private Button button(String text) {
        Button b = new Button(this);
        b.setText(text);
        b.setAllCaps(false);
        b.setTextSize(14f);
        b.setMinWidth(0);
        b.setMinimumWidth(0);
        b.setPadding(18, 4, 18, 4);
        return b;
    }

    private void configureWebView() {
        WebSettings s = webView.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setDatabaseEnabled(true);
        s.setLoadsImagesAutomatically(true);
        s.setCacheMode(WebSettings.LOAD_DEFAULT);
        s.setBuiltInZoomControls(true);
        s.setDisplayZoomControls(false);
        s.setSupportZoom(true);
        s.setUseWideViewPort(true);
        s.setLoadWithOverviewMode(true);
        s.setMediaPlaybackRequiresUserGesture(true);

        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(webView, true);

        webView.addJavascriptInterface(new Object() {
            @JavascriptInterface
            public void openImage(String url) {
                if (url == null || !(url.startsWith("https://") || url.startsWith("http://"))) return;
                runOnUiThread(() -> showImage(url));
            }
        }, "DaViewer");

        webView.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onProgressChanged(WebView view, int newProgress) {
                progress.setProgress(newProgress);
                progress.setVisibility(newProgress >= 100 ? View.GONE : View.VISIBLE);
            }
        });

        webView.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                Uri uri = request.getUrl();
                String scheme = uri.getScheme();
                return !("http".equalsIgnoreCase(scheme) || "https".equalsIgnoreCase(scheme));
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                CookieManager.getInstance().flush();
                if (currentImageUrl == null) {
                    galleryUrl = url;
                    getPreferences(MODE_PRIVATE).edit().putString("last_gallery", url).apply();
                    injectImageTapHandler();
                }
            }
        });

        webView.setDownloadListener((url, userAgent, contentDisposition, mimetype, contentLength) ->
                download(url));

        webView.setOnLongClickListener(v -> {
            WebView.HitTestResult hit = webView.getHitTestResult();
            if (hit != null && (hit.getType() == WebView.HitTestResult.IMAGE_TYPE
                    || hit.getType() == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE)) {
                String url = hit.getExtra();
                if (url != null && url.startsWith("http")) {
                    showImage(url);
                    return true;
                }
            }
            return false;
        });
    }

    private void injectImageTapHandler() {
        String js =
                "(function(){" +
                "if(window.__daviewerInstalled)return;" +
                "window.__daviewerInstalled=true;" +
                "document.addEventListener('click',function(e){" +
                "var img=e.target.closest?e.target.closest('img'):null;" +
                "if(!img)return;" +
                "var u=img.currentSrc||img.src;" +
                "if(!u||u.indexOf('http')!==0)return;" +
                "e.preventDefault();e.stopPropagation();" +
                "DaViewer.openImage(u);" +
                "},true);" +
                "})();";
        webView.evaluateJavascript(js, null);
    }

    private void openGallery(String url) {
        currentImageUrl = null;
        galleryUrl = url;
        galleryButton.setEnabled(false);
        downloadButton.setEnabled(false);
        webView.getSettings().setLoadWithOverviewMode(false);
        webView.loadUrl(url);
    }

    private void showImage(String imageUrl) {
        if (currentImageUrl == null && webView.getUrl() != null) {
            galleryUrl = webView.getUrl();
        }
        currentImageUrl = imageUrl;
        galleryButton.setEnabled(true);
        downloadButton.setEnabled(true);
        webView.getSettings().setLoadWithOverviewMode(true);

        String escaped = imageUrl
                .replace("&", "&amp;")
                .replace(""", "&quot;")
                .replace("<", "&lt;")
                .replace(">", "&gt;");

        String html =
                "<!doctype html><html><head>" +
                "<meta name='viewport' content='width=device-width,initial-scale=1,maximum-scale=5,user-scalable=yes'>" +
                "<style>html,body{margin:0;background:#000;width:100%;height:100%;display:flex;align-items:center;justify-content:center;overflow:auto}" +
                "img{max-width:100%;height:auto;display:block;margin:auto}</style></head>" +
                "<body><img src=\"" + escaped + "\"></body></html>";

        String base = galleryUrl != null ? galleryUrl : imageUrl;
        webView.loadDataWithBaseURL(base, html, "text/html", "UTF-8", null);
    }

    private void returnToGallery() {
        currentImageUrl = null;
        galleryButton.setEnabled(false);
        downloadButton.setEnabled(false);
        webView.getSettings().setLoadWithOverviewMode(false);

        if (webView.canGoBack()) {
            webView.goBack();
        } else if (galleryUrl != null) {
            webView.loadUrl(galleryUrl);
        }
    }

    private void download(String url) {
        if (url == null || !(url.startsWith("http://") || url.startsWith("https://"))) {
            Toast.makeText(this, "No downloadable image selected", Toast.LENGTH_SHORT).show();
            return;
        }

        try {
            String fileName = URLUtil.guessFileName(url, null, "image/*");
            DownloadManager.Request request = new DownloadManager.Request(Uri.parse(url));
            request.setTitle(fileName);
            request.setDescription("DaViewer image");
            request.setNotificationVisibility(
                    DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED);
            request.setAllowedOverMetered(true);
            request.setAllowedOverRoaming(true);
            request.addRequestHeader("User-Agent", webView.getSettings().getUserAgentString());

            String cookie = CookieManager.getInstance().getCookie(url);
            if (cookie != null) request.addRequestHeader("Cookie", cookie);
            if (galleryUrl != null) request.addRequestHeader("Referer", galleryUrl);

            request.setDestinationInExternalPublicDir(
                    Environment.DIRECTORY_DOWNLOADS,
                    "DaViewer/" + fileName);

            DownloadManager dm =
                    (DownloadManager) getSystemService(Context.DOWNLOAD_SERVICE);
            dm.enqueue(request);
            Toast.makeText(this, "Saved to Downloads/DaViewer", Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(this, "Download failed", Toast.LENGTH_LONG).show();
        }
    }

    @Override
    protected void onPause() {
        CookieManager.getInstance().flush();
        webView.onPause();
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        webView.onResume();
    }

    @Override
    protected void onDestroy() {
        webView.destroy();
        super.onDestroy();
    }

    @Override
    public void onBackPressed() {
        if (currentImageUrl != null) {
            returnToGallery();
        } else if (webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
