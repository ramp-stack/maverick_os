package me.ramp.maverick;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.Context;
import android.content.Intent;
import java.util.UUID;

final class MaverickNotifications {
    static void createChannel(Context context) {
        context.getSystemService(NotificationManager.class).createNotificationChannel(
            new NotificationChannel("maverick", "Notifications", NotificationManager.IMPORTANCE_DEFAULT));
    }

    static void post(Context context, String title, String body, String tag) {
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (!manager.areNotificationsEnabled()) return;
        createChannel(context);
        Notification.Builder notification = new Notification.Builder(context, "maverick")
            .setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle(title)
            .setContentText(body).setAutoCancel(true);
        Intent launch = context.getPackageManager().getLaunchIntentForPackage(context.getPackageName());
        if (launch != null) notification.setContentIntent(PendingIntent.getActivity(context, 0, launch,
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE));
        manager.notify(tag == null ? UUID.randomUUID().toString() : tag, 0, notification.build());
    }
}
