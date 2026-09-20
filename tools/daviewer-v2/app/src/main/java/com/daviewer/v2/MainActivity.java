package com.daviewer.v2;

import android.app.Activity;
import android.app.DownloadManager;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Color;
import android.graphics.Matrix;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Environment;
import android.os.Handler;
import android.os.Looper;
import android.util.LruCache;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.View;
import android.view.ViewGroup;
import android.webkit.CookieManager;
import android.webkit.JavascriptInterface;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.BaseAdapter;
import android.widget.Button;
import android.widget.EditText;
import android.widget.FrameLayout;
import android.widget.GridView;
import android.widget.HorizontalScrollView;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;
import android.widget.Toast;

import org.json.JSONArray;

import com.bumptech.glide.Glide;
import com.bumptech.glide.load.engine.DiskCacheStrategy;

import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLDecoder;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public class MainActivity extends Activity {

    private static final int BG = Color.rgb(12, 12, 14);
    private static final int SURFACE = Color.rgb(25, 25, 29);
    private static final int SURFACE_2 = Color.rgb(38, 38, 44);
    private static final int TEXT = Color.WHITE;
    private static final int MUTED = Color.rgb(175, 175, 185);

    private final Map<String, String> sources = new LinkedHashMap<>();
    private final LinkedHashSet<String> imageSet = new LinkedHashSet<>();
    private final List<String> imageUrls = new ArrayList<>();
    private final Handler main = new Handler(Looper.getMainLooper());
    private final ExecutorService imagePool = Executors.newFixedThreadPool(6);

    private FrameLayout root;
    private LinearLayout browserScreen;
    private WebView collector;
    private GridView grid;
    private ImageAdapter adapter;
    private EditText search;
    private ProgressBar topProgress;
    private TextView status;
    private LinearLayout chips;
    private FrameLayout viewerOverlay;
    private ZoomImageView fullImage;
    private ProgressBar imageProgress;
    private Button downloadButton;

    private String activeSource = "DeviantArt";
    private String currentPageUrl;
    private String currentImageUrl;
    private boolean loadMorePending = false;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        sources.put("DeviantArt", "https://www.deviantart.com/");
        sources.put("ArtStation", "https://www.artstation.com/channels/all?sort_by=community");
        sources.put("Behance", "https://www.behance.net/galleries");
        sources.put("Pixiv", "https://www.pixiv.net/en/");
        sources.put("Flickr", "https://www.flickr.com/explore");

        getWindow().setStatusBarColor(BG);
        getWindow().setNavigationBarColor(BG);

        root = new FrameLayout(this);
        root.setBackgroundColor(BG);

        browserScreen = new LinearLayout(this);
        browserScreen.setOrientation(LinearLayout.VERTICAL);
        browserScreen.setBackgroundColor(BG);
        root.addView(browserScreen, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));

        buildHeader();
        buildGrid();
        buildCollector();
        buildViewer();

        setContentView(root);

        String savedSource = getPreferences(MODE_PRIVATE)
                .getString("source", "DeviantArt");
        if (sources.containsKey(savedSource)) activeSource = savedSource;
        selectSource(activeSource, false);
    }

    private void buildHeader() {
        LinearLayout top = new LinearLayout(this);
        top.setOrientation(LinearLayout.VERTICAL);
        top.setPadding(dp(12), dp(8), dp(12), dp(8));
        top.setBackgroundColor(SURFACE);

        LinearLayout titleRow = new LinearLayout(this);
        titleRow.setOrientation(LinearLayout.HORIZONTAL);
        titleRow.setGravity(Gravity.CENTER_VERTICAL);

        TextView title = new TextView(this);
        title.setText("DaViewer");
        title.setTextColor(TEXT);
        title.setTextSize(TypedValue.COMPLEX_UNIT_SP, 22);
        title.setTypeface(null, 1);
        titleRow.addView(title, new LinearLayout.LayoutParams(0, dp(42), 1f));

        TextView hint = new TextView(this);
        hint.setText("images only");
        hint.setTextColor(MUTED);
        hint.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        hint.setGravity(Gravity.CENTER_VERTICAL | Gravity.END);
        titleRow.addView(hint, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(42)));

        top.addView(titleRow);

        HorizontalScrollView chipScroll = new HorizontalScrollView(this);
        chipScroll.setHorizontalScrollBarEnabled(false);
        chips = new LinearLayout(this);
        chips.setOrientation(LinearLayout.HORIZONTAL);
        chips.setPadding(0, dp(2), 0, dp(6));
        chipScroll.addView(chips);

        for (String name : sources.keySet()) {
            TextView chip = makeChip(name);
            chip.setTag(name);
            chip.setOnClickListener(v -> selectSource((String) v.getTag(), true));
            chips.addView(chip);
        }
        top.addView(chipScroll);

        LinearLayout searchRow = new LinearLayout(this);
        searchRow.setOrientation(LinearLayout.HORIZONTAL);
        searchRow.setGravity(Gravity.CENTER_VERTICAL);

        search = new EditText(this);
        search.setSingleLine(true);
        search.setHint("Search " + activeSource);
        search.setHintTextColor(Color.rgb(130, 130, 140));
        search.setTextColor(TEXT);
        search.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        search.setPadding(dp(14), 0, dp(14), 0);
        search.setBackground(roundRect(SURFACE_2, dp(14)));
        search.setOnEditorActionListener((v, actionId, event) -> {
            runSearch();
            return true;
        });
        searchRow.addView(search, new LinearLayout.LayoutParams(0, dp(48), 1f));

        Button go = new Button(this);
        go.setText("Search");
        go.setAllCaps(false);
        go.setTextColor(TEXT);
        go.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14);
        go.setBackground(roundRect(Color.rgb(69, 69, 82), dp(14)));
        go.setOnClickListener(v -> runSearch());
        LinearLayout.LayoutParams goLp = new LinearLayout.LayoutParams(dp(92), dp(48));
        goLp.setMargins(dp(8), 0, 0, 0);
        searchRow.addView(go, goLp);

        top.addView(searchRow);

        status = new TextView(this);
        status.setTextColor(MUTED);
        status.setTextSize(TypedValue.COMPLEX_UNIT_SP, 12);
        status.setPadding(dp(2), dp(7), 0, 0);
        status.setText("Loading images…");
        top.addView(status);

        browserScreen.addView(top);

        topProgress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        topProgress.setMax(100);
        browserScreen.addView(topProgress, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, dp(3)));
    }

    private void buildGrid() {
        grid = new GridView(this);
        grid.setNumColumns(2);
        grid.setHorizontalSpacing(dp(5));
        grid.setVerticalSpacing(dp(5));
        grid.setPadding(dp(5), dp(5), dp(5), dp(12));
        grid.setClipToPadding(false);
        grid.setStretchMode(GridView.STRETCH_COLUMN_WIDTH);
        grid.setBackgroundColor(BG);
        grid.setSelector(android.R.color.transparent);

        adapter = new ImageAdapter();
        grid.setAdapter(adapter);
        grid.setOnItemClickListener((parent, view, position, id) -> {
            if (position >= 0 && position < imageUrls.size()) {
                openViewer(imageUrls.get(position));
            }
        });

        grid.setOnScrollListener(new android.widget.AbsListView.OnScrollListener() {
            @Override
            public void onScrollStateChanged(android.widget.AbsListView view, int scrollState) {}

            @Override
            public void onScroll(android.widget.AbsListView view, int firstVisibleItem,
                                 int visibleItemCount, int totalItemCount) {
                if (totalItemCount > 0 &&
                        firstVisibleItem + visibleItemCount >= totalItemCount - 6) {
                    loadMore();
                }
            }
        });

        browserScreen.addView(grid, new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f));
    }

    private void buildCollector() {
        collector = new WebView(this);
        collector.setBackgroundColor(Color.TRANSPARENT);
        collector.setAlpha(0.01f);

        WebSettings s = collector.getSettings();
        s.setJavaScriptEnabled(true);
        s.setDomStorageEnabled(true);
        s.setDatabaseEnabled(true);
        s.setLoadsImagesAutomatically(true);
        s.setCacheMode(WebSettings.LOAD_DEFAULT);
        s.setMediaPlaybackRequiresUserGesture(true);
        s.setUserAgentString(s.getUserAgentString() + " DaViewer/4.0");

        CookieManager.getInstance().setAcceptCookie(true);
        CookieManager.getInstance().setAcceptThirdPartyCookies(collector, true);

        collector.addJavascriptInterface(new ImageBridge(), "DaViewerImages");

        collector.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onProgressChanged(WebView view, int newProgress) {
                topProgress.setProgress(newProgress);
                topProgress.setVisibility(newProgress >= 100 ? View.GONE : View.VISIBLE);
            }
        });

        collector.setWebViewClient(new WebViewClient() {
            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                String scheme = request.getUrl().getScheme();
                return !("http".equalsIgnoreCase(scheme) || "https".equalsIgnoreCase(scheme));
            }

            @Override
            public void onPageFinished(WebView view, String url) {
                currentPageUrl = url;
                CookieManager.getInstance().flush();
                main.postDelayed(MainActivity.this::extractImages, 450);
                main.postDelayed(MainActivity.this::extractImages, 1400);
                main.postDelayed(MainActivity.this::extractImages, 2800);
            }
        });

        FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT);
        root.addView(collector, 0, lp);
    }

    private void buildViewer() {
        viewerOverlay = new FrameLayout(this);
        viewerOverlay.setBackgroundColor(Color.BLACK);
        viewerOverlay.setVisibility(View.GONE);

        fullImage = new ZoomImageView(this);
        fullImage.setBackgroundColor(Color.BLACK);
        viewerOverlay.addView(fullImage, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));

        imageProgress = new ProgressBar(this);
        FrameLayout.LayoutParams progressLp = new FrameLayout.LayoutParams(
                dp(52), dp(52), Gravity.CENTER);
        viewerOverlay.addView(imageProgress, progressLp);

        LinearLayout bottom = new LinearLayout(this);
        bottom.setOrientation(LinearLayout.HORIZONTAL);
        bottom.setPadding(dp(10), dp(8), dp(10), dp(10));
        bottom.setBackgroundColor(Color.argb(225, 18, 18, 20));

        Button back = viewerButton("‹ Gallery");
        back.setOnClickListener(v -> closeViewer());
        bottom.addView(back, new LinearLayout.LayoutParams(0, dp(54), 1f));

        downloadButton = viewerButton("⬇ Download");
        downloadButton.setOnClickListener(v -> downloadCurrentImage());
        LinearLayout.LayoutParams dlLp = new LinearLayout.LayoutParams(0, dp(54), 1f);
        dlLp.setMargins(dp(8), 0, 0, 0);
        bottom.addView(downloadButton, dlLp);

        FrameLayout.LayoutParams bottomLp = new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
                Gravity.BOTTOM);
        viewerOverlay.addView(bottom, bottomLp);

        root.addView(viewerOverlay, new FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
    }

    private TextView makeChip(String name) {
        TextView t = new TextView(this);
        t.setText(name);
        t.setTextColor(TEXT);
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, 13);
        t.setGravity(Gravity.CENTER);
        t.setPadding(dp(15), 0, dp(15), 0);
        t.setBackground(roundRect(SURFACE_2, dp(18)));
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT, dp(38));
        lp.setMargins(0, 0, dp(7), 0);
        t.setLayoutParams(lp);
        return t;
    }

    private Button viewerButton(String text) {
        Button b = new Button(this);
        b.setText(text);
        b.setAllCaps(false);
        b.setTextColor(TEXT);
        b.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        b.setBackground(roundRect(Color.rgb(43, 43, 49), dp(14)));
        return b;
    }

    private void updateChipSelection() {
        for (int i = 0; i < chips.getChildCount(); i++) {
            View child = chips.getChildAt(i);
            String name = (String) child.getTag();
            if (child instanceof TextView) {
                ((TextView) child).setBackground(roundRect(
                        activeSource.equals(name)
                                ? Color.rgb(74, 74, 92)
                                : SURFACE_2,
                        dp(18)));
            }
        }
    }

    private void selectSource(String source, boolean clearSearch) {
        activeSource = source;
        getPreferences(MODE_PRIVATE).edit().putString("source", source).apply();
        if (clearSearch) search.setText("");
        search.setHint("Search " + source);
        updateChipSelection();
        clearImages();
        String url = sources.get(source);
        status.setText("Loading " + source + "…");
        currentPageUrl = url;
        collector.loadUrl(url);
    }

    private void runSearch() {
        String q = search.getText().toString().trim();
        if (q.isEmpty()) {
            selectSource(activeSource, false);
            return;
        }

        clearImages();
        String enc = Uri.encode(q);
        String url;
        switch (activeSource) {
            case "ArtStation":
                url = "https://www.artstation.com/search?sort_by=relevance&query=" + enc;
                break;
            case "Behance":
                url = "https://www.behance.net/search/projects?search=" + enc;
                break;
            case "Pixiv":
                url = "https://www.pixiv.net/en/tags/" + enc + "/artworks";
                break;
            case "Flickr":
                url = "https://www.flickr.com/search/?text=" + enc;
                break;
            case "DeviantArt":
            default:
                url = "https://www.deviantart.com/search?q=" + enc;
                break;
        }

        status.setText("Searching " + activeSource + "…");
        currentPageUrl = url;
        collector.loadUrl(url);
    }

    private void clearImages() {
        imageSet.clear();
        imageUrls.clear();
        adapter.notifyDataSetChanged();
        loadMorePending = false;
        status.setText("Loading images…");
    }

    private void extractImages() {
        String js =
                "(function(){" +
                "var out=[];" +
                "function add(u,el){" +
                "if(!u)return;u=(''+u).trim();" +
                "if(!/^https?:/i.test(u))return;" +
                "if(/favicon|sprite|emoji|logo|badge|avatar/i.test(u) && el && Math.max(el.clientWidth||0,el.clientHeight||0)<450)return;" +
                "out.push(u);" +
                "}" +
                "function best(s){if(!s)return '';var p=s.split(',');return p[p.length-1].trim().split(/\\s+/)[0]||'';}" +
                "document.querySelectorAll('img').forEach(function(im){" +
                "var w=im.naturalWidth||im.clientWidth||0,h=im.naturalHeight||im.clientHeight||0;" +
                "if(w>0&&h>0&&w<120&&h<120)return;" +
                "add(best(im.getAttribute('srcset')),im);" +
                "add(im.currentSrc,im);" +
                "add(im.getAttribute('data-src'),im);" +
                "add(im.getAttribute('data-original'),im);" +
                "add(im.getAttribute('data-lazy-src'),im);" +
                "add(im.src,im);" +
                "});" +
                "document.querySelectorAll('source[srcset]').forEach(function(s){add(best(s.getAttribute('srcset')),s);});" +
                "document.querySelectorAll('[style*=background-image]').forEach(function(el){" +
                "var b=getComputedStyle(el).backgroundImage||'';var m=b.match(/url\\([\\\"']?(.*?)[\\\"']?\\)/);if(m)add(m[1],el);" +
                "});" +
                "if(out.length)DaViewerImages.addImages(JSON.stringify(out));" +
                "})();";
        collector.evaluateJavascript(js, null);
    }

    private void loadMore() {
        if (loadMorePending || collector == null) return;
        loadMorePending = true;
        status.setText(imageUrls.size() + " images · loading more…");

        String js =
                "(function(){" +
                "window.scrollBy(0, Math.max(window.innerHeight*2, 1400));" +
                "setTimeout(function(){window.scrollBy(0, Math.max(window.innerHeight*2, 1400));},500);" +
                "})();";
        collector.evaluateJavascript(js, null);

        main.postDelayed(() -> {
            extractImages();
            loadMorePending = false;
        }, 1300);
        main.postDelayed(this::extractImages, 2600);
    }

    private void openViewer(String url) {
        currentImageUrl = url;
        fullImage.resetZoom();
        fullImage.setImageDrawable(null);
        viewerOverlay.setVisibility(View.VISIBLE);
        imageProgress.setVisibility(View.VISIBLE);
        downloadButton.setEnabled(true);

        Glide.with(this)
                .asBitmap()
                .load(url)
                .diskCacheStrategy(DiskCacheStrategy.AUTOMATIC)
                .into(new com.bumptech.glide.request.target.CustomTarget<Bitmap>() {
                    @Override
                    public void onResourceReady(
                            Bitmap resource,
                            com.bumptech.glide.request.transition.Transition<? super Bitmap> transition) {
                        if (!url.equals(currentImageUrl)) return;
                        imageProgress.setVisibility(View.GONE);
                        fullImage.setImageBitmap(resource);
                    }

                    @Override
                    public void onLoadCleared(android.graphics.drawable.Drawable placeholder) {
                    }

                    @Override
                    public void onLoadFailed(android.graphics.drawable.Drawable errorDrawable) {
                        imageProgress.setVisibility(View.GONE);
                        Toast.makeText(MainActivity.this, "Could not load full image", Toast.LENGTH_SHORT).show();
                    }
                });
    }

    private void closeViewer() {
        currentImageUrl = null;
        fullImage.setImageDrawable(null);
        viewerOverlay.setVisibility(View.GONE);
    }

    private void downloadCurrentImage() {
        if (currentImageUrl == null) return;
        try {
            Uri uri = Uri.parse(currentImageUrl);
            String name = uri.getLastPathSegment();
            if (name == null || name.length() < 3) name = "daviewer_" + System.currentTimeMillis() + ".jpg";
            try {
                name = URLDecoder.decode(name, "UTF-8");
            } catch (Exception ignored) {}
            if (!name.contains(".")) name += ".jpg";
            name = name.replaceAll("[^a-zA-Z0-9._-]", "_");

            DownloadManager.Request request = new DownloadManager.Request(uri);
            request.setTitle(name);
            request.setDescription("DaViewer image");
            request.setNotificationVisibility(
                    DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED);
            request.setAllowedOverMetered(true);
            request.setAllowedOverRoaming(true);

            String cookie = CookieManager.getInstance().getCookie(currentImageUrl);
            if (cookie != null) request.addRequestHeader("Cookie", cookie);
            request.addRequestHeader("User-Agent", collector.getSettings().getUserAgentString());
            if (currentPageUrl != null) request.addRequestHeader("Referer", currentPageUrl);

            request.setDestinationInExternalPublicDir(
                    Environment.DIRECTORY_DOWNLOADS,
                    "DaViewer/" + name);

            DownloadManager dm = (DownloadManager) getSystemService(Context.DOWNLOAD_SERVICE);
            dm.enqueue(request);
            Toast.makeText(this, "Saved to Downloads/DaViewer", Toast.LENGTH_SHORT).show();
        } catch (Exception e) {
            Toast.makeText(this, "Download failed", Toast.LENGTH_SHORT).show();
        }
    }

    private Bitmap loadBitmap(String urlString) {
        HttpURLConnection c = null;
        InputStream in = null;
        try {
            c = (HttpURLConnection) new URL(urlString).openConnection();
            c.setConnectTimeout(12000);
            c.setReadTimeout(22000);
            c.setInstanceFollowRedirects(true);
            c.setRequestProperty("User-Agent", collector.getSettings().getUserAgentString());
            c.setRequestProperty("Accept", "image/avif,image/webp,image/apng,image/svg+xml,image/*,*/*;q=0.8");
            String cookie = CookieManager.getInstance().getCookie(urlString);
            if (cookie != null) c.setRequestProperty("Cookie", cookie);
            if (currentPageUrl != null) c.setRequestProperty("Referer", currentPageUrl);
            c.connect();
            int code = c.getResponseCode();
            if (code < 200 || code >= 400) return null;
            in = c.getInputStream();
            return BitmapFactory.decodeStream(in);
        } catch (Exception e) {
            return null;
        } finally {
            try { if (in != null) in.close(); } catch (Exception ignored) {}
            if (c != null) c.disconnect();
        }
    }

    private class ImageBridge {
        @JavascriptInterface
        public void addImages(String json) {
            try {
                JSONArray arr = new JSONArray(json);
                List<String> fresh = new ArrayList<>();
                synchronized (imageSet) {
                    for (int i = 0; i < arr.length(); i++) {
                        String u = arr.optString(i, null);
                        if (u == null || !(u.startsWith("http://") || u.startsWith("https://"))) continue;
                        if (imageSet.add(u)) fresh.add(u);
                    }
                }

                if (!fresh.isEmpty()) {
                    main.post(() -> {
                        imageUrls.addAll(fresh);
                        adapter.notifyDataSetChanged();
                        status.setText(imageUrls.size() + " images · scroll for more");
                    });
                } else {
                    main.post(() -> {
                        if (imageUrls.isEmpty()) status.setText("Still looking for images…");
                        else status.setText(imageUrls.size() + " images");
                    });
                }
            } catch (Exception ignored) {}
        }
    }

    private class ImageAdapter extends BaseAdapter {
        @Override
        public int getCount() {
            return imageUrls.size();
        }

        @Override
        public Object getItem(int position) {
            return imageUrls.get(position);
        }

        @Override
        public long getItemId(int position) {
            return position;
        }

        @Override
        public View getView(int position, View convertView, ViewGroup parent) {
            ImageView iv;
            if (convertView instanceof ImageView) {
                iv = (ImageView) convertView;
            } else {
                iv = new ImageView(MainActivity.this);
                iv.setScaleType(ImageView.ScaleType.CENTER_CROP);
                iv.setBackgroundColor(Color.rgb(30, 30, 34));
                iv.setLayoutParams(new GridView.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, dp(220)));
            }

            String url = imageUrls.get(position);
            iv.setTag(url);
            Glide.with(MainActivity.this)
                    .load(url)
                    .diskCacheStrategy(DiskCacheStrategy.AUTOMATIC)
                    .thumbnail(0.2f)
                    .centerCrop()
                    .into(iv);
            return iv;
        }
    }

    private static class ZoomImageView extends ImageView {
        private final Matrix matrix = new Matrix();
        private final ScaleGestureDetector scaleDetector;
        private float scale = 1f;
        private float lastX;
        private float lastY;
        private boolean dragging;

        ZoomImageView(Context context) {
            super(context);
            setScaleType(ScaleType.MATRIX);
            setImageMatrix(matrix);

            scaleDetector = new ScaleGestureDetector(context,
                    new ScaleGestureDetector.SimpleOnScaleGestureListener() {
                        @Override
                        public boolean onScale(ScaleGestureDetector detector) {
                            float factor = detector.getScaleFactor();
                            float target = scale * factor;
                            if (target < 1f) factor = 1f / scale;
                            if (target > 5f) factor = 5f / scale;
                            scale *= factor;
                            matrix.postScale(factor, factor,
                                    detector.getFocusX(), detector.getFocusY());
                            setImageMatrix(matrix);
                            return true;
                        }
                    });
        }

        void resetZoom() {
            scale = 1f;
            matrix.reset();
            setImageMatrix(matrix);
        }

        @Override
        public boolean onTouchEvent(MotionEvent event) {
            scaleDetector.onTouchEvent(event);

            switch (event.getActionMasked()) {
                case MotionEvent.ACTION_DOWN:
                    lastX = event.getX();
                    lastY = event.getY();
                    dragging = true;
                    break;
                case MotionEvent.ACTION_MOVE:
                    if (dragging && !scaleDetector.isInProgress() && scale > 1f) {
                        float dx = event.getX() - lastX;
                        float dy = event.getY() - lastY;
                        matrix.postTranslate(dx, dy);
                        setImageMatrix(matrix);
                        lastX = event.getX();
                        lastY = event.getY();
                    }
                    break;
                case MotionEvent.ACTION_UP:
                case MotionEvent.ACTION_CANCEL:
                    dragging = false;
                    break;
            }
            return true;
        }

        @Override
        protected void onSizeChanged(int w, int h, int oldw, int oldh) {
            super.onSizeChanged(w, h, oldw, oldh);
            fitCenter();
        }

        @Override
        public void setImageBitmap(Bitmap bm) {
            super.setImageBitmap(bm);
            post(this::fitCenter);
        }

        private void fitCenter() {
            if (getDrawable() == null || getWidth() <= 0 || getHeight() <= 0) return;
            float dw = getDrawable().getIntrinsicWidth();
            float dh = getDrawable().getIntrinsicHeight();
            if (dw <= 0 || dh <= 0) return;

            float fit = Math.min((float) getWidth() / dw, (float) getHeight() / dh);
            float dx = (getWidth() - dw * fit) / 2f;
            float dy = (getHeight() - dh * fit) / 2f;

            matrix.reset();
            matrix.postScale(fit, fit);
            matrix.postTranslate(dx, dy);
            scale = 1f;
            setImageMatrix(matrix);
        }
    }

    private GradientDrawable roundRect(int color, int radius) {
        GradientDrawable d = new GradientDrawable();
        d.setColor(color);
        d.setCornerRadius(radius);
        return d;
    }

    private int dp(int value) {
        return (int) TypedValue.applyDimension(
                TypedValue.COMPLEX_UNIT_DIP,
                value,
                getResources().getDisplayMetrics());
    }

    @Override
    public void onBackPressed() {
        if (viewerOverlay.getVisibility() == View.VISIBLE) {
            closeViewer();
            return;
        }
        if (collector.canGoBack()) {
            collector.goBack();
            return;
        }
        super.onBackPressed();
    }

    @Override
    protected void onPause() {
        CookieManager.getInstance().flush();
        collector.onPause();
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        collector.onResume();
    }

    @Override
    protected void onDestroy() {
        imagePool.shutdownNow();
        collector.destroy();
        super.onDestroy();
    }
}
