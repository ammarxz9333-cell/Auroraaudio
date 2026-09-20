package com.daviewer.v2;

import android.app.Activity;
import android.app.AlertDialog;
import android.app.DownloadManager;
import android.content.Context;
import android.content.Intent;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.DownloadListener;
import android.webkit.URLUtil;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.Button;
import android.widget.EditText;
import android.widget.HorizontalScrollView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.Toast;

import java.util.LinkedHashMap;
import java.util.Map;

public class MainActivity extends Activity {
    private WebView webView;
    private EditText address;
    private ProgressBar progress;
    private final Map<String, String> sources = new LinkedHashMap<>();

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
        root.setBackgroundColor(Color.rgb(18, 18, 18));

        HorizontalScrollView sourceScroll = new HorizontalScrollView(this);
        sourceScroll.setHorizontalScrollBarEnabled(false);
        LinearLayout sourceBar = new LinearLayout(this);
        sourceBar.setOrientation(LinearLayout.HORIZONTAL);
        sourceBar.setPadding(6, 6, 6, 6);
        for (Map.Entry<String, String> entry : sources.entrySet()) {
            Button b = compactButton(entry.getKey());
            b.setOnClickListener(v -> load(entry.getValue()));
            sourceBar.addView(b);
        }
        Button mature = compactButton("18+");
        mature.setOnClickListener(v -> showMatureInfo());
        sourceBar.addView(mature);
        sourceScroll.addView(sourceBar);
        root.addView(sourceScroll, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));

        LinearLayout nav = new LinearLayout(this);
        nav.setOrientation(LinearLayout.HORIZONTAL);
        nav.setGravity(Gravity.CENTER_VERTICAL);
        nav.setPadding(6, 2, 6, 4);

        Button back = compactButton("‹");
        back.setOnClickListener(v -> { if (webView.canGoBack()) webView.goBack(); });
        nav.addView(back);

        Button forward = compactButton("›");
        forward.setOnClickListener(v -> { if (webView.canGoForward()) webView.goForward(); });
        nav.addView(forward);

        Button reload = compactButton("↻");
        reload.setOnClickListener(v -> webView.reload());
        nav.addView(reload);

        Button home = compactButton("⌂");
        home.setOnClickListener(v -> load(sources.get("DeviantArt")));
        nav.addView(home);

        address = new EditText(this);
        address.setSingleLine(true);
        address.setTextColor(Color.WHITE);
        address.setHintTextColor(Color.GRAY);
        address.setHint("URL or search");
        address.setBackgroundColor(Color.rgb(40, 40, 40));
        LinearLayout.LayoutParams addressLp = new LinearLayout.LayoutParams(0, 48, 1f);
        addressLp.setMargins(6, 0, 6, 0);
        nav.addView(address, addressLp);

        Button go = compactButton("GO");
        go.setOnClickListener(v -> navigateAddress());
        nav.addView(go);
        root.addView(nav);

        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        root.addView(progress, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 4));

        webView = new WebView(this);
        root.addView(webView, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));
        setContentView(root);

        configureWebView();

        String last = getPreferences(MODE_PRIVATE)
                .getString("last_url", sources.get("DeviantArt"));
        load(last);
    }

    private Button compactButton(String text) {
        Button b = new Button(this);
        b.setText(text);
        b.setAllCaps(false);
        b.setTextSize(13f);
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
        s.setJavaScriptCanOpenWindowsAutomatically(true);
        s.setSupportMultipleWindows(false);
        s.setMediaPlaybackRequiresUserGesture(true);

        CookieManager cookies = CookieManager.getInstance();
        cookies.setAcceptCookie(true);
        cookies.setAcceptThirdPartyCookies(webView, true);

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
                if ("http".equalsIgnoreCase(scheme) || "https".equalsIgnoreCase(scheme)) {
                    return false;
                }
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, uri));
                } catch (Exception e) {
                    Toast.makeText(MainActivity.this, "Cannot open this link", Toast.LENGTH_SHORT).show();
                }
                return true;
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                address.setText(url);
                getPreferences(MODE_PRIVATE).edit().putString("last_url", url).apply();
                CookieManager.getInstance().flush();
            }
        });

        webView.setDownloadListener((url, userAgent, contentDisposition, mimetype, contentLength) ->
                download(url, userAgent, contentDisposition, mimetype));

        webView.setOnLongClickListener(v -> {
            WebView.HitTestResult hit = webView.getHitTestResult();
            if (hit != null && (hit.getType() == WebView.HitTestResult.IMAGE_TYPE
                    || hit.getType() == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE)) {
                String imageUrl = hit.getExtra();
                if (imageUrl != null && imageUrl.startsWith("http")) {
                    download(imageUrl, webView.getSettings().getUserAgentString(), null, "image/*");
                    return true;
                }
            }
            return false;
        });
    }

    private void navigateAddress() {
        String value = address.getText().toString().trim();
        if (value.isEmpty()) return;
        if (value.startsWith("http://") || value.startsWith("https://")) {
            load(value);
        } else if (value.contains(".") && !value.contains(" ")) {
            load("https://" + value);
        } else {
            load("https://www.deviantart.com/search?q=" + Uri.encode(value));
        }
    }

    private void load(String url) {
        if (url == null || url.isEmpty()) return;
        webView.loadUrl(url);
    }

    private void showMatureInfo() {
        new AlertDialog.Builder(this)
                .setTitle("Mature / Sensitive content")
                .setMessage("DaViewer does not bypass age gates, subscriptions, private posts, or permissions. Sign in to each source and enable Mature/Sensitive content in that site's own settings. DaViewer keeps the login cookies so your allowed content remains visible.")
                .setPositiveButton("OK", null)
                .show();
    }

    private void download(String url, String userAgent, String contentDisposition, String mimeType) {
        if (url == null || !(url.startsWith("http://") || url.startsWith("https://"))) {
            Toast.makeText(this, "This item cannot be downloaded directly", Toast.LENGTH_SHORT).show();
            return;
        }
        try {
            String fileName = URLUtil.guessFileName(url, contentDisposition, mimeType);
            DownloadManager.Request request = new DownloadManager.Request(Uri.parse(url));
            request.setTitle(fileName);
            request.setDescription("Downloading with DaViewer");
            request.setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED);
            request.setAllowedOverMetered(true);
            request.setAllowedOverRoaming(true);
            if (mimeType != null) request.setMimeType(mimeType);
            if (userAgent != null) request.addRequestHeader("User-Agent", userAgent);
            String cookie = CookieManager.getInstance().getCookie(url);
            if (cookie != null) request.addRequestHeader("Cookie", cookie);
            request.setDestinationInExternalPublicDir(Environment.DIRECTORY_DOWNLOADS, "DaViewer/" + fileName);

            DownloadManager dm = (DownloadManager) getSystemService(Context.DOWNLOAD_SERVICE);
            dm.enqueue(request);
            Toast.makeText(this, "Download started", Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(this, "Download failed: " + e.getMessage(), Toast.LENGTH_LONG).show();
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
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            super.onBackPressed();
        }
    }
}
