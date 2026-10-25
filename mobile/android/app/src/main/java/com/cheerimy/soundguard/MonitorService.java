package com.cheerimy.soundguard;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;
import android.util.Log;

import org.json.JSONObject;

import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.net.HttpURLConnection;
import java.net.URL;
import java.net.URLEncoder;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.OffsetDateTime;
import java.time.format.DateTimeParseException;
import java.util.Locale;

public class MonitorService extends Service {
    private static final String TAG = "SoundGuardMonitor";
    private static final String PREFS = MainActivity.PREFS;
    private static final int SERVICE_NOTIFICATION_ID = 7001;
    private static final int ALERT_NOTIFICATION_ID = 7010;
    private static final long POLL_INTERVAL_MS = 5000L;
    private static final long FALLBACK_COOLDOWN_MS = 5 * 60 * 1000L;
    private static final long FRESH_ALERT_MS = 15 * 60 * 1000L;

    private volatile boolean running = true;
    private Thread worker;
    private SharedPreferences prefs;
    private String serverUrl = "";
    private String token = "";
    private String deviceId = "";
    private String deviceName = "";
    private double threshold = 65.0;
    private String lastAlertId = "";
    private long lastFallbackAt = 0L;
    private String lastServiceText = "";

    @Override
    public void onCreate() {
        super.onCreate();
        createChannel();
        prefs = getSharedPreferences(PREFS, MODE_PRIVATE);
        loadSession();
        lastAlertId = prefs.getString("last_alert_id", "");
        lastFallbackAt = prefs.getLong("last_fallback_at", 0L);

        Notification notification = buildServiceNotification("正在连接声垣服务器…");
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            int foregroundType = Build.VERSION.SDK_INT >= 34
                    ? ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
                    : ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC;
            startForeground(SERVICE_NOTIFICATION_ID, notification, foregroundType);
        } else {
            startForeground(SERVICE_NOTIFICATION_ID, notification);
        }
        startPolling();
    }

    private synchronized void loadSession() {
        serverUrl = prefs.getString("server_url", "");
        token = prefs.getString("token", "");
        deviceId = prefs.getString("device_id", "");
        deviceName = prefs.getString("device_name", "声垣");
        try {
            threshold = Double.parseDouble(prefs.getString("threshold", "65"));
        } catch (NumberFormatException ignored) {
            threshold = 65.0;
        }
    }

    private void createChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return;
        }
        NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        NotificationChannel service = new NotificationChannel(
                MainActivity.CHANNEL_SERVICE, "后台监听", NotificationManager.IMPORTANCE_LOW);
        service.setDescription("保持与声垣服务器的连接，在后台接收超标提醒。");
        service.setShowBadge(false);
        manager.createNotificationChannel(service);

        NotificationChannel alert = new NotificationChannel(
                MainActivity.CHANNEL_ALERT, "分贝超标提醒", NotificationManager.IMPORTANCE_HIGH);
        alert.setDescription("设备环境声音超过阈值时立即提醒。");
        alert.enableVibration(true);
        manager.createNotificationChannel(alert);
    }

    private Notification buildServiceNotification(String text) {
        lastServiceText = text;
        Intent intent = new Intent(this, MainActivity.class);
        intent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent pending = PendingIntent.getActivity(
                this, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);

        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, MainActivity.CHANNEL_SERVICE);
        } else {
            builder = new Notification.Builder(this);
        }
        return builder
                .setSmallIcon(R.drawable.ic_stat_sound)
                .setContentTitle("声垣 · " + deviceName)
                .setContentText(text)
                .setStyle(new Notification.BigTextStyle().bigText(text))
                .setContentIntent(pending)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setCategory(Notification.CATEGORY_SERVICE)
                .build();
    }

    private void updateServiceNotification(String text) {
        if (text.equals(lastServiceText)) {
            return;
        }
        NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        manager.notify(SERVICE_NOTIFICATION_ID, buildServiceNotification(text));
    }

    private void startPolling() {
        worker = new Thread(() -> {
            while (running) {
                long sleepMs = POLL_INTERVAL_MS;
                try {
                    if (serverUrl.isEmpty() || token.isEmpty() || deviceId.isEmpty()) {
                        updateServiceNotification("尚未登录，请打开 App 完成登录。");
                    } else {
                        Status status = fetchStatus();
                        if (status.unauthorized) {
                            updateServiceNotification("账号无权限或登录已失效，请打开 App。");
                            showSessionExpiredNotification();
                            stopSelf();
                            return;
                        }
                        if (status.ok) {
                            handleStatus(status);
                        } else {
                            updateServiceNotification("后台监听中 · 网络暂时不可用，正在重试…");
                        }
                    }
                } catch (Exception e) {
                    Log.w(TAG, "background poll failed", e);
                    updateServiceNotification("后台监听中 · 网络暂时不可用，正在重试…");
                    sleepMs = 8000L;
                }
                try {
                    Thread.sleep(sleepMs);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                    return;
                }
            }
        }, "soundguard-monitor");
        worker.setDaemon(true);
        worker.start();
    }

    private Status fetchStatus() throws Exception {
        final String requestServer, requestToken, requestDevice;
        synchronized (this) { requestServer = serverUrl; requestToken = token; requestDevice = deviceId; }
        String encoded = URLEncoder.encode(requestDevice, "UTF-8");
        HttpURLConnection connection = (HttpURLConnection) new URL(
                requestServer + "/api/devices/" + encoded + "/status").openConnection();
        connection.setConnectTimeout(6000);
        connection.setReadTimeout(7000);
        connection.setRequestMethod("GET");
        connection.setInstanceFollowRedirects(false);
        connection.setRequestProperty("Accept", "application/json");
        connection.setRequestProperty("Authorization", "Bearer " + requestToken);
        try {
            int code = connection.getResponseCode();
            synchronized (this) {
                if (!requestServer.equals(serverUrl) || !requestToken.equals(token) || !requestDevice.equals(deviceId)) return new Status();
            }
            if (code == 401 || code == 403) {
                Status status = new Status();
                status.unauthorized = true;
                return status;
            }
            if (code != 200) {
                return new Status();
            }
            String body = readBody(connection.getInputStream());
            Status status = new Status();
            status.server = requestServer;
            status.token = requestToken;
            status.device = requestDevice;
            status.ok = true;
            status.json = new JSONObject(body);
            return status;
        } finally {
            connection.disconnect();
        }
    }

    private String readBody(InputStream stream) throws Exception {
        StringBuilder builder = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(
                new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                builder.append(line);
            }
        }
        return builder.toString();
    }

    private synchronized void handleStatus(Status status) {
        if (!serverUrl.equals(status.server) || !token.equals(status.token) || !deviceId.equals(status.device)) return;
        JSONObject root = status.json;
        JSONObject device = root.optJSONObject("device");
        if (device == null) {
            return;
        }
        String name = device.optString("name", deviceName);
        if (!name.isEmpty() && !name.equals(deviceName)) {
            deviceName = name;
            prefs.edit().putString("device_name", name).apply();
        }
        double configuredThreshold = root.optDouble("thresholdDb", threshold);
        if (!Double.isNaN(configuredThreshold) && configuredThreshold > 0) {
            threshold = configuredThreshold;
            prefs.edit().putString("threshold", String.valueOf(threshold)).apply();
        }
        boolean warActive = root.optBoolean("warActive", false);
        boolean captureWindow = root.optBoolean("captureWindowActive", true);
        boolean online = device.optBoolean("online", false);
        double current = device.optDouble("currentDb", Double.NaN);
        double average = device.optDouble("liveAvgDb", current);
        boolean suspended = device.optBoolean("liveSuspended", false);
        boolean monitoring = device.optBoolean("monitoring", false);
        boolean ignoringSystem = device.optBoolean("liveSystemPlaying", false)
                && !device.optBoolean("liveAecEnabled", false);

        JSONObject alert = root.optJSONObject("latestNoiseAlert");
        String alertId = alert == null ? "" : alert.optString("id", "");
        double alertDb = alert == null ? 0 : alert.optDouble("db", 0);
        String alertMessage = alert == null ? "" : alert.optString("message", "");
        String alertAt = alert == null ? "" : alert.optString("createdAt", "");

        if (!alertId.isEmpty() && !alertId.equals(lastAlertId)) {
            if (!warActive && captureWindow && isFresh(alertAt, FRESH_ALERT_MS)) {
                showOverLimitNotification(name, alertDb, alertMessage);
            }
            lastAlertId = alertId;
            lastFallbackAt = System.currentTimeMillis();
            prefs.edit()
                    .putString("last_alert_id", lastAlertId)
                    .putLong("last_fallback_at", lastFallbackAt)
                    .apply();
        } else if (!warActive && captureWindow && online && monitoring && !suspended && !ignoringSystem
                && !Double.isNaN(average) && average >= threshold
                && System.currentTimeMillis() - lastFallbackAt > FALLBACK_COOLDOWN_MS) {
            lastFallbackAt = System.currentTimeMillis();
            prefs.edit().putLong("last_fallback_at", lastFallbackAt).apply();
            showOverLimitNotification(name,
                    Double.isNaN(average) ? current : average, "");
        }

        if (!online) {
            updateServiceNotification("设备暂时离线 · 等待采集端重新连接");
        } else if (warActive) {
            updateServiceNotification("分贝战争进行中 · 本时段不触发噪声提醒");
        } else if (!captureWindow && device.optBoolean("desiredMonitoring", false)) {
            updateServiceNotification("计划待机 · 等待指定采集时段");
        } else if (!monitoring) {
            updateServiceNotification("采集已暂停 · 等待恢复");
        } else if (suspended || ignoringSystem) {
            updateServiceNotification("通话中 · 分贝监测暂停");
        } else if (!Double.isNaN(average)) {
            updateServiceNotification(String.format(Locale.CHINA,
                    "后台监听中 · 平均值 %.1f dB · 阈值 %.1f dB", average, threshold));
        } else {
            updateServiceNotification("后台监听中 · 等待实时数据");
        }
    }

    private boolean isFresh(String isoTime, long maxAgeMs) {
        if (isoTime == null || isoTime.isEmpty()) {
            return false;
        }
        try {
            Instant created = OffsetDateTime.parse(isoTime).toInstant();
            long age = System.currentTimeMillis() - created.toEpochMilli();
            return age <= maxAgeMs && age >= -60_000L;
        } catch (DateTimeParseException ignored) {
            return false;
        }
    }

    private void showSessionExpiredNotification() {
        Intent intent = new Intent(this, MainActivity.class);
        intent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent pending = PendingIntent.getActivity(
                this, 11, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, MainActivity.CHANNEL_ALERT);
        } else {
            builder = new Notification.Builder(this);
        }
        Notification notification = builder
                .setSmallIcon(R.drawable.ic_stat_sound)
                .setContentTitle("声垣 · 登录需要更新")
                .setContentText("请打开 App 重新登录，或确认账号仍有设备权限。")
                .setStyle(new Notification.BigTextStyle()
                        .bigText("请打开 App 重新登录，或确认账号仍有设备权限。"))
                .setContentIntent(pending)
                .setAutoCancel(true)
                .setPriority(Notification.PRIORITY_HIGH)
                .setCategory(Notification.CATEGORY_ERROR)
                .build();
        NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        manager.notify(7011, notification);
    }

    private void showOverLimitNotification(String name, double db, String message) {
        Intent intent = new Intent(this, MainActivity.class);
        intent.setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent pending = PendingIntent.getActivity(
                this, 10, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);

        String text;
        if (message != null && !message.isEmpty()) {
            text = message;
        } else if (!Double.isNaN(db) && db > 0) {
            text = String.format(Locale.CHINA,
                    "环境声音平均值 %.1f dB，超过阈值 %.1f dB。", db, threshold);
        } else {
            text = String.format(Locale.CHINA,
                    "环境声音超过阈值 %.1f dB，请及时关注。", threshold);
        }

        Notification.Builder builder;
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder = new Notification.Builder(this, MainActivity.CHANNEL_ALERT);
        } else {
            builder = new Notification.Builder(this);
        }
        Notification notification = builder
                .setSmallIcon(R.drawable.ic_stat_sound)
                .setContentTitle(name + " · 环境声音超标")
                .setContentText(text)
                .setStyle(new Notification.BigTextStyle().bigText(text))
                .setContentIntent(pending)
                .setAutoCancel(true)
                .setPriority(Notification.PRIORITY_HIGH)
                .setCategory(Notification.CATEGORY_ALARM)
                .setDefaults(Notification.DEFAULT_ALL)
                .build();
        NotificationManager manager = (NotificationManager) getSystemService(Context.NOTIFICATION_SERVICE);
        manager.notify(ALERT_NOTIFICATION_ID, notification);

        prefs.edit().putString("last_alert_id", lastAlertId).apply();
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        loadSession();
        return START_STICKY;
    }

    @Override
    public void onDestroy() {
        running = false;
        if (worker != null) {
            worker.interrupt();
        }
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private static class Status {
        String server, token, device;
        boolean ok;
        boolean unauthorized;
        JSONObject json;
    }
}
