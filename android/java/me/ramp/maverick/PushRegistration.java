package me.ramp.maverick;

import android.os.Handler;
import android.os.Looper;
import android.os.ParcelFileDescriptor;
import android.util.Log;
import com.google.firebase.FirebaseApp;
import com.google.firebase.messaging.FirebaseMessaging;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Iterator;
import java.util.concurrent.atomic.AtomicInteger;

final class PushRegistration {
    private final MaverickActivity activity;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private final AtomicInteger next = new AtomicInteger(1);
    private final HashMap<Integer, Waiter> waiters = new HashMap<>();
    private String current;
    private boolean requesting;
    private boolean closed;
    private int generation;
    private long delay = 1000;

    PushRegistration(MaverickActivity activity) {this.activity = activity;}

    long begin(String previous) throws IOException {
        int id = next.getAndIncrement();
        ParcelFileDescriptor[] pair = ParcelFileDescriptor.createSocketPair();
        int fd = pair[0].detachFd();
        handler.post(() -> {
            Waiter waiter = new Waiter(pair[1], previous);
            if (closed) {waiter.close(); return;}
            waiters.put(id, waiter);
            activity.requestNotificationPermission();
            complete();
            request();
        });
        return ((long)id << 32) | (fd & 0xffffffffL);
    }

    void cancel(int id) {
        handler.post(() -> {
            Waiter waiter = waiters.remove(id);
            if (waiter != null) waiter.close();
        });
    }

    void token(String token) {
        handler.post(() -> {
            if (closed) return;
            generation++;
            current = token;
            delay = 1000;
            complete();
        });
    }

    private void complete() {
        if (current == null) return;
        Iterator<Waiter> pending = waiters.values().iterator();
        while (pending.hasNext()) {
            Waiter waiter = pending.next();
            if (current.equals(waiter.previous)) continue;
            pending.remove();
            try {waiter.output.write(current.getBytes(StandardCharsets.UTF_8));}
            catch (IOException error) { /* The native waiter was cancelled. */ }
            finally {waiter.close();}
        }
    }

    private void request() {
        if (closed || requesting || current != null || waiters.isEmpty()) return;
        if (FirebaseApp.getApps(activity).isEmpty()) {
            Log.w("Maverick", "Push registration requires Firebase Android app configuration");
            return;
        }
        requesting = true;
        int started = generation;
        FirebaseMessaging.getInstance().getToken().addOnCompleteListener(task -> {
            requesting = false;
            if (closed) return;
            if (task.isSuccessful()) {
                if (started == generation) token(task.getResult());
                return;
            }
            Log.w("Maverick", "FCM token registration failed; retrying");
            handler.postDelayed(() -> request(), delay);
            delay = Math.min(delay * 2, 60000);
        });
    }

    void close() {
        closed = true;
        handler.removeCallbacksAndMessages(null);
        for (Waiter waiter : waiters.values()) waiter.close();
        waiters.clear();
    }

    private static final class Waiter {
        final ParcelFileDescriptor.AutoCloseOutputStream output;
        final String previous;
        Waiter(ParcelFileDescriptor fd, String previous) {
            output = new ParcelFileDescriptor.AutoCloseOutputStream(fd);
            this.previous = previous;
        }
        void close() {
            try {output.close();}
            catch (IOException error) {Log.w("Maverick", "Push waiter close failed", error);}
        }
    }
}
