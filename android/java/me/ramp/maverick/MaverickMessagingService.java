package me.ramp.maverick;

import com.google.firebase.messaging.FirebaseMessagingService;
import com.google.firebase.messaging.RemoteMessage;

public final class MaverickMessagingService extends FirebaseMessagingService {
    @Override public void onNewToken(String token) {
        MaverickActivity activity = MaverickActivity.current();
        if (activity != null) activity.pushToken(token);
    }

    @Override public void onMessageReceived(RemoteMessage message) {
        RemoteMessage.Notification notification = message.getNotification();
        if (notification == null) return;
        String title = notification.getTitle() == null ? "New activity" : notification.getTitle();
        String body = notification.getBody() == null ? "You have a notification." : notification.getBody();
        MaverickActivity activity = MaverickActivity.current();
        if (activity != null) activity.notifyUser(title, body);
        else MaverickNotifications.post(this, title, body, notification.getTag());
    }
}
