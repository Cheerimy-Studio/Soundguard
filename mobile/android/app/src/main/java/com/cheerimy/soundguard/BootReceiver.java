package com.cheerimy.soundguard;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.os.Build;

public class BootReceiver extends BroadcastReceiver {
    @Override
    public void onReceive(Context context, Intent intent) {
        if (intent == null || !Intent.ACTION_BOOT_COMPLETED.equals(intent.getAction())) {
            return;
        }
        SharedPreferences prefs = context.getSharedPreferences(MainActivity.PREFS, Context.MODE_PRIVATE);
        String token = prefs.getString("token", "");
        String deviceId = prefs.getString("device_id", "");
        if (token.isEmpty() || deviceId.isEmpty()) {
            return;
        }
        try {
            Intent service = new Intent(context, MonitorService.class);
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(service);
            } else {
                context.startService(service);
            }
        } catch (Exception ignored) {
            // Android may defer foreground-service starts after boot; opening the app restarts it.
        }
    }
}
