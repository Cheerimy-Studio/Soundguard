package com.cheerimy.soundguard;

import android.Manifest;
import android.annotation.SuppressLint;
import android.app.Activity;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.PowerManager;
import android.provider.Settings;
import android.view.View;
import android.webkit.JavascriptInterface;
import android.webkit.PermissionRequest;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceRequest;
import android.webkit.WebResourceResponse;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.widget.Toast;

import androidx.webkit.WebViewAssetLoader;
import androidx.webkit.WebViewClientCompat;

public class MainActivity extends Activity {
    static final String PREFS = "soundguard_mobile";
    static final String CHANNEL_SERVICE = "soundguard_service";
    static final String CHANNEL_ALERT = "soundguard_alert";

    private WebView webView;

    @SuppressLint("SetJavaScriptEnabled")
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        createChannels();

        webView = new WebView(this);
        webView.setBackgroundColor(0xFFF5F7F8);
        setContentView(webView);

        WebSettings settings = webView.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setDatabaseEnabled(true);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setAllowFileAccess(false);
        settings.setAllowContentAccess(false);
        settings.setMixedContentMode(WebSettings.MIXED_CONTENT_ALWAYS_ALLOW);
        settings.setCacheMode(WebSettings.LOAD_DEFAULT);
        settings.setUserAgentString(settings.getUserAgentString() + " SoundGuardMobile/0.1.0");

        if (BuildConfig.DEBUG) {
            WebView.setWebContentsDebuggingEnabled(true);
        }

        webView.addJavascriptInterface(new NativeBridge(), "Android");

        webView.setWebChromeClient(new WebChromeClient() {
            @Override
            public void onPermissionRequest(final PermissionRequest request) {
                runOnUiThread(() -> {
                    boolean micGranted = checkSelfPermission(Manifest.permission.RECORD_AUDIO)
                            == PackageManager.PERMISSION_GRANTED;
                    if (!micGranted) {
                        request.deny();
                        requestRuntimePermissions();
                        return;
                    }
                    String[] resources = request.getResources();
                    boolean wantsMic = false;
                    for (String resource : resources) {
                        if (PermissionRequest.RESOURCE_AUDIO_CAPTURE.equals(resource)) {
                            wantsMic = true;
                            break;
                        }
                    }
                    if (wantsMic) {
                        request.grant(new String[]{PermissionRequest.RESOURCE_AUDIO_CAPTURE});
                    } else {
                        request.deny();
                    }
                });
            }
        });

        final WebViewAssetLoader assetLoader = new WebViewAssetLoader.Builder()
                .addPathHandler("/assets/", new WebViewAssetLoader.AssetsPathHandler(this))
                .build();

        webView.setWebViewClient(new WebViewClientCompat() {
            @Override
            public WebResourceResponse shouldInterceptRequest(WebView view, WebResourceRequest request) {
                return assetLoader.shouldInterceptRequest(request.getUrl());
            }

            @Override
            public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                String url = request.getUrl().toString();
                if (url.startsWith("https://appassets.androidplatform.net/")) {
                    return false;
                }
                try {
                    startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
                } catch (Exception ignored) {
                }
                return true;
            }
        });

        webView.loadUrl("https://appassets.androidplatform.net/assets/index.html");
        requestRuntimePermissions();
    }

    private void createChannels() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return;
        }
        NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        NotificationChannel service = new NotificationChannel(
                CHANNEL_SERVICE, "后台监听", NotificationManager.IMPORTANCE_LOW);
        service.setDescription("保持与声垣服务器的连接，在后台接收超标提醒。");
        service.setShowBadge(false);
        manager.createNotificationChannel(service);

        NotificationChannel alert = new NotificationChannel(
                CHANNEL_ALERT, "分贝超标提醒", NotificationManager.IMPORTANCE_HIGH);
        alert.setDescription("设备环境声音超过阈值时立即提醒。");
        alert.enableVibration(true);
        manager.createNotificationChannel(alert);
    }

    private void requestRuntimePermissions() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            requestPermissions(new String[]{
                    Manifest.permission.RECORD_AUDIO,
                    Manifest.permission.POST_NOTIFICATIONS
            }, 1001);
        } else {
            requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, 1001);
        }
    }

    private void startMonitorService() {
        SharedPreferences prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        if (prefs.getString("token", "").isEmpty() || prefs.getString("device_id", "").isEmpty()) {
            return;
        }
        Intent intent = new Intent(this, MonitorService.class);
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(intent);
            } else {
                startService(intent);
            }
        } catch (Exception e) {
            Toast.makeText(this, "后台监听启动失败：" + e.getMessage(), Toast.LENGTH_SHORT).show();
        }
    }

    private void stopMonitorService() {
        stopService(new Intent(this, MonitorService.class));
    }

    @Override
    public void onBackPressed() {
        if (webView != null && webView.canGoBack()) {
            webView.goBack();
        } else {
            moveTaskToBack(true);
        }
    }

    @Override
    protected void onDestroy() {
        if (webView != null) {
            webView.destroy();
            webView = null;
        }
        super.onDestroy();
    }

    private String normalizeServer(String serverUrl) {
        if (serverUrl == null) {
            return "";
        }
        String value = serverUrl.trim();
        while (value.endsWith("/")) {
            value = value.substring(0, value.length() - 1);
        }
        return value;
    }

    public class NativeBridge {
        @JavascriptInterface
        public void saveSession(String serverUrl, String token, String deviceId,
                                String deviceName, double threshold) {
            SharedPreferences.Editor editor = getSharedPreferences(PREFS, MODE_PRIVATE).edit();
            editor.putString("server_url", normalizeServer(serverUrl));
            editor.putString("token", token == null ? "" : token);
            editor.putString("device_id", deviceId == null ? "" : deviceId);
            editor.putString("device_name", deviceName == null ? "" : deviceName);
            editor.putString("threshold", String.valueOf(threshold));
            editor.putBoolean("has_session", true);
            editor.apply();
        }

        @JavascriptInterface
        public void startMonitor() {
            runOnUiThread(MainActivity.this::startMonitorService);
        }

        @JavascriptInterface
        public void stopMonitor() {
            runOnUiThread(MainActivity.this::stopMonitorService);
        }

        @JavascriptInterface
        public void clearSession() {
            getSharedPreferences(PREFS, MODE_PRIVATE).edit().clear().apply();
            runOnUiThread(MainActivity.this::stopMonitorService);
        }

        @JavascriptInterface
        public String getSavedServer() {
            return getSharedPreferences(PREFS, MODE_PRIVATE).getString("server_url", "");
        }

        @JavascriptInterface
        public String getSavedToken() {
            return getSharedPreferences(PREFS, MODE_PRIVATE).getString("token", "");
        }

        @JavascriptInterface
        public String getSavedDeviceId() {
            return getSharedPreferences(PREFS, MODE_PRIVATE).getString("device_id", "");
        }

        @JavascriptInterface
        public String getSavedDeviceName() {
            return getSharedPreferences(PREFS, MODE_PRIVATE).getString("device_name", "");
        }

        @JavascriptInterface
        public boolean isIgnoringBatteryOptimizations() {
            PowerManager manager = (PowerManager) getSystemService(Context.POWER_SERVICE);
            return manager != null && manager.isIgnoringBatteryOptimizations(getPackageName());
        }

        @JavascriptInterface
        public void openBatterySettings() {
            runOnUiThread(() -> {
                try {
                    Intent intent = new Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS);
                    intent.setData(Uri.parse("package:" + getPackageName()));
                    startActivity(intent);
                } catch (Exception e) {
                    try {
                        startActivity(new Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS));
                    } catch (Exception ignored) {
                    }
                }
            });
        }

        @JavascriptInterface
        public void openNotificationSettings() {
            runOnUiThread(() -> {
                try {
                    Intent intent = new Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS);
                    intent.putExtra(Settings.EXTRA_APP_PACKAGE, getPackageName());
                    startActivity(intent);
                } catch (Exception ignored) {
                }
            });
        }

        @JavascriptInterface
        public boolean notificationsEnabled() {
            NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
            return manager != null && manager.areNotificationsEnabled();
        }

        @JavascriptInterface
        public void requestNotificationPermission() {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                runOnUiThread(() -> requestPermissions(
                        new String[]{Manifest.permission.POST_NOTIFICATIONS}, 1002));
            }
        }

        @JavascriptInterface
        public void showToast(String message) {
            runOnUiThread(() -> Toast.makeText(MainActivity.this, message, Toast.LENGTH_SHORT).show());
        }
    }
}
