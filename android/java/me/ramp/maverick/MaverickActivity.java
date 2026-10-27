package me.ramp.maverick;

import android.Manifest;
import android.app.NativeActivity;
import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.content.BroadcastReceiver;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.IntentFilter;
import android.content.pm.PackageManager;
import android.graphics.Bitmap;
import android.graphics.ImageDecoder;
import android.media.AudioAttributes;
import android.media.AudioFocusRequest;
import android.media.AudioManager;
import android.media.session.MediaSession;
import android.media.session.PlaybackState;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.util.Log;
import android.view.HapticFeedbackConstants;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsAnimation;
import android.widget.TextView;
import org.json.JSONArray;
import org.json.JSONObject;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.UUID;
import java.util.concurrent.atomic.AtomicInteger;

public final class MaverickActivity extends NativeActivity {
    private static volatile java.lang.ref.WeakReference<MaverickActivity> current = new java.lang.ref.WeakReference<>(null);
    public static MaverickActivity current() {return current.get();}
    private final AtomicInteger nextPick = new AtomicInteger(1);
    private final HashMap<Integer, PendingPick> picks = new HashMap<>();
    private final ArrayList<String[]> notifications = new ArrayList<>();
    private final ArrayList<String[]> pendingNotifications = new ArrayList<>();
    private volatile float[] insets = new float[4];
    private final AtomicInteger nextKeyboard = new AtomicInteger(1);
    private KeyboardView keyboard;
    private boolean keyboardDragging;
    private long keyboardDragPointer;
    private float keyboardDragX;
    private float keyboardDragY;
    private long events;
    private boolean observingEvents;
    private volatile boolean keyboardVisible;
    private PushRegistration push;
    private MaverickCapture capture;
    private MediaSession mediaSession;
    private final HashMap<String, Boolean> players = new HashMap<>();
    private AudioFocusRequest audioFocus;
    private int audioUsers;
    private BroadcastReceiver audioRoutes;
    private boolean resumed;
    private boolean notificationPermissionRequested;
    private boolean notificationPermissionWanted;
    private volatile float keyboardInset;
    private WindowInsets animatedInsets;
    private android.graphics.Rect windowBounds;
    private final HashMap<WindowInsetsAnimation, Boolean> imeAnimations = new HashMap<>();

    private native void mediaCommand(int command);
    private native void windowChanged();
    private native void keyboardText(String text);
    private native boolean keyboardKey(int code, int unicode, int action, int repeat, int meta);
    private native void notificationReceived(String title, String body);

    public void observeEvents() {
        runOnUiThread(() -> {
            synchronized (this) {
                if (events == 0 || isDestroyed()) return;
                observingEvents = true;
                startMedia();
                updateInsets();
                getWindow().getDecorView().requestApplyInsets();
                flushNotifications();
            }
        });
    }

    public synchronized void stopEvents() {
        observingEvents = false;
        runOnUiThread(() -> {
            if (mediaSession != null) {mediaSession.release(); mediaSession = null;}
            if (audioRoutes != null) {unregisterReceiver(audioRoutes); audioRoutes = null;}
            getSystemService(NotificationManager.class).cancel("maverick.media", 0);
        });
        KeyboardView closing = keyboard;
        runOnUiThread(() -> {
            synchronized (this) {
                if (closing == null || keyboard != closing) return;
                keyboardDragging = false;
                closing.close();
                keyboard = null;
            }
        });
    }

    synchronized void sendText(String text) {
        if (observingEvents && events != 0) keyboardText(text);
    }

    synchronized boolean sendKey(int code, int unicode, int action, int repeat, int meta) {
        return observingEvents && events != 0 && keyboardKey(code, unicode, action, repeat, meta);
    }

    private synchronized void flushNotifications() {
        if (!resumed || !observingEvents || events == 0) return;
        for (String[] notification : pendingNotifications) {
            notificationReceived(notification[0], notification[1]);
        }
        pendingNotifications.clear();
    }

    public int openKeyboard() {
        int id = nextKeyboard.getAndIncrement();
        runOnUiThread(() -> {
            synchronized (this) {
                if (!observingEvents || events == 0) return;
                if (keyboard != null) keyboard.close();
                keyboardDragging = false;
                keyboard = new KeyboardView(this, id);
                addContentView(keyboard, new ViewGroup.LayoutParams(1, 1));
                keyboard.requestFocus();
                keyboard.post(() -> {
                    synchronized (this) {
                        if (observingEvents && events != 0 && keyboard != null && keyboard.id == id) {
                            keyboard.show();
                        }
                    }
                });
            }
        });
        return id;
    }

    public void reopenKeyboard(int id) {
        runOnUiThread(() -> {
            if (keyboard == null || keyboard.id != id) return;
            keyboardDragging = false;
            WindowInsets value = getWindow().getDecorView().getRootWindowInsets();
            if (!keyboard.isFocused() || value == null || !value.isVisible(WindowInsets.Type.ime())) {
                keyboard.setFocusableInTouchMode(true);
                keyboard.requestFocus();
                keyboard.show();
            }
        });
    }

    public void hideKeyboard(int id) {
        runOnUiThread(() -> {
            if (keyboard == null || keyboard.id != id) return;
            keyboardDragging = false;
            keyboard.hide();
        });
    }

    public void closeKeyboard(int id) {
        runOnUiThread(() -> {
            synchronized (this) {
                if (keyboard == null || keyboard.id != id) return;
                keyboardDragging = false;
                keyboard.close();
                keyboard = null;
            }
        });
    }

    public void keyboardTouch(int action, long pointer, float x, float y, int touches) {
        runOnUiThread(() -> {
            if (keyboard == null) return;
            WindowInsets value = imeAnimations.isEmpty() ?
                getWindow().getDecorView().getRootWindowInsets() : animatedInsets;
            if (value == null || (!value.isVisible(WindowInsets.Type.ime())
                && value.getInsets(WindowInsets.Type.ime()).bottom == 0)) return;
            if (action == MotionEvent.ACTION_DOWN) {
                keyboardDragging = touches == 1
                    && !keyboardDragBlocked(getWindow().getDecorView(), x, y);
                if (keyboardDragging) {
                    keyboardDragPointer = pointer;
                    keyboardDragX = x;
                    keyboardDragY = y;
                }
            } else if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
                keyboardDragging = false;
            } else if (keyboardDragging && pointer == keyboardDragPointer) {
                float dx = x - keyboardDragX, dy = y - keyboardDragY;
                if (dy >= 32 * getResources().getDisplayMetrics().density && dy > Math.abs(dx)) {
                    keyboardDragging = false;
                    keyboard.hide();
                }
            }
        });
    }

    @Override public void onCreate(Bundle state) {
        current = new java.lang.ref.WeakReference<>(this);
        push = new PushRegistration(this);
        MaverickNotifications.createChannel(this);
        super.onCreate(state);
        windowBounds = getWindowManager().getCurrentWindowMetrics().getBounds();
        View decor = getWindow().getDecorView();
        decor.setOnApplyWindowInsetsListener((view, value) -> {
            if (imeAnimations.isEmpty()) updateInsets(value);
            return view.onApplyWindowInsets(value);
        });
        decor.setWindowInsetsAnimationCallback(new WindowInsetsAnimation.Callback(
            WindowInsetsAnimation.Callback.DISPATCH_MODE_CONTINUE_ON_SUBTREE) {
            @Override public void onPrepare(WindowInsetsAnimation animation) {
                if ((animation.getTypeMask() & WindowInsets.Type.ime()) != 0) {
                    if (imeAnimations.isEmpty()) animatedInsets = decor.getRootWindowInsets();
                    imeAnimations.put(animation, false);
                }
            }
            @Override public WindowInsetsAnimation.Bounds onStart(WindowInsetsAnimation animation, WindowInsetsAnimation.Bounds bounds) {
                if ((animation.getTypeMask() & WindowInsets.Type.ime()) != 0) {
                    imeAnimations.put(animation, true);
                }
                return bounds;
            }
            @Override public WindowInsets onProgress(WindowInsets value, java.util.List<WindowInsetsAnimation> running) {
                animatedInsets = value;
                updateInsets(value);
                return value;
            }
            @Override public void onEnd(WindowInsetsAnimation animation) {
                if (imeAnimations.remove(animation) == null) return;
                if (!imeAnimations.containsValue(true)) {
                    imeAnimations.clear();
                    animatedInsets = null;
                    updateInsets();
                }
            }
        });
        decor.getViewTreeObserver().addOnGlobalLayoutListener(() -> updateInsets());
    }

    @Override protected void onResume() {
        super.onResume();
        resumed = true;
        flushNotifications();
        if (notificationPermissionWanted) requestNotificationPermission();
    }

    @Override protected void onPause() {
        resumed = false;
        keyboardDragging = false;
        super.onPause();
    }

    private synchronized void sendMedia(int command) {
        if (observingEvents && events != 0) mediaCommand(command);
    }

    private void startMedia() {
        if (mediaSession != null) return;
        mediaSession = new MediaSession(this, "Maverick");
        mediaSession.setCallback(new MediaSession.Callback() {
            @Override public void onPlay() {sendMedia(0);}
            @Override public void onPause() {sendMedia(1);}
            @Override public void onStop() {sendMedia(1);}
        });
        audioRoutes = new BroadcastReceiver() {
            @Override public void onReceive(Context context, Intent intent) {
                sendMedia(intent.getAction().equals(getPackageName() + ".maverick.PLAY") ? 0 : 1);
            }
        };
        IntentFilter filter = new IntentFilter(
            AudioManager.ACTION_AUDIO_BECOMING_NOISY);
        filter.addAction(getPackageName() + ".maverick.PLAY");
        filter.addAction(getPackageName() + ".maverick.PAUSE");
        if (android.os.Build.VERSION.SDK_INT >= 33) registerReceiver(audioRoutes, filter, RECEIVER_NOT_EXPORTED);
        else registerReceiver(audioRoutes, filter);
        publishPlayback();
    }

    public void playback(String id, int state) {
        runOnUiThread(() -> {
            if (state < 0) {
                if (players.remove(id) == null) return;
            } else {
                Boolean previous = players.put(id, state != 0);
                if (previous != null && previous == (state != 0)) return;
            }
            publishPlayback();
        });
    }

    private void publishPlayback() {
        if (mediaSession == null) return;
        boolean playing = players.containsValue(true);
        int state = players.isEmpty() ? PlaybackState.STATE_STOPPED :
            playing ? PlaybackState.STATE_PLAYING :
            PlaybackState.STATE_PAUSED;
        mediaSession.setPlaybackState(new PlaybackState.Builder()
            .setActions(PlaybackState.ACTION_PLAY |
                PlaybackState.ACTION_PAUSE |
                PlaybackState.ACTION_PLAY_PAUSE |
                PlaybackState.ACTION_STOP)
            .setState(state, PlaybackState.PLAYBACK_POSITION_UNKNOWN,
                playing ? 1.0f : 0.0f).build());
        mediaSession.setActive(!players.isEmpty());
        NotificationManager manager = getSystemService(NotificationManager.class);
        if (players.isEmpty()) {manager.cancel("maverick.media", 0); return;}
        manager.createNotificationChannel(new NotificationChannel(
            "maverick.media", "Playback", NotificationManager.IMPORTANCE_LOW));
        Intent action = new Intent(getPackageName() + (playing ? ".maverick.PAUSE" : ".maverick.PLAY"))
            .setPackage(getPackageName());
        PendingIntent pending = PendingIntent.getBroadcast(this, 0, action,
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification.Builder notification = new Notification.Builder(this, "maverick.media")
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(getApplicationInfo().loadLabel(getPackageManager()))
            .setCategory(Notification.CATEGORY_TRANSPORT).setOnlyAlertOnce(true)
            .setShowWhen(false).setOngoing(playing)
            .setStyle(new Notification.MediaStyle().setMediaSession(mediaSession.getSessionToken())
                .setShowActionsInCompactView(0))
            .addAction(playing ? android.R.drawable.ic_media_pause : android.R.drawable.ic_media_play,
                playing ? "Pause" : "Play", pending);
        Intent launch = getPackageManager().getLaunchIntentForPackage(getPackageName());
        if (launch != null) notification.setContentIntent(PendingIntent.getActivity(this, 0,
            launch, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE));
        manager.notify("maverick.media", 0, notification.build());
    }

    public synchronized String acquireAudio() {
        if (audioUsers > 0) {audioUsers++; return null;}
        AudioManager manager = getSystemService(AudioManager.class);
        audioFocus = new AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(new AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_MEDIA)
                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setWillPauseWhenDucked(true)
            .setOnAudioFocusChangeListener(change -> {
                if (change < 0) sendMedia(1);
            }, new Handler(getMainLooper())).build();
        if (manager.requestAudioFocus(audioFocus) != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            audioFocus = null;
            return "Audio focus is unavailable";
        }
        audioUsers = 1;
        return null;
    }

    public synchronized void releaseAudio() {
        if (--audioUsers == 0) {
            getSystemService(AudioManager.class).abandonAudioFocusRequest(audioFocus);
            audioFocus = null;
        }
    }

    public MaverickCapture createCapture() {
        capture = new MaverickCapture(this);
        return capture;
    }

    public long beginPush(String previous) throws java.io.IOException {return push.begin(previous);}
    public void cancelPush(int id) {push.cancel(id);}
    void pushToken(String token) {push.token(token);}

    void requestNotificationPermission() {
        notificationPermissionWanted = true;
        if (!resumed || notificationPermissionRequested || android.os.Build.VERSION.SDK_INT < 33
            || checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED) return;
        notificationPermissionRequested = true;
        requestPermissions(new String[] {Manifest.permission.POST_NOTIFICATIONS}, 60001);
    }

    private boolean keyboardDragBlocked(View view, float x, float y) {
        if (!view.isShown()) return false;
        int[] position = new int[2];
        view.getLocationInWindow(position);
        if (x < position[0] || x >= position[0] + view.getWidth()
            || y < position[1] || y >= position[1] + view.getHeight()) return false;
        if (view instanceof TextView || view.isClickable() || view.isLongClickable()
            || view.onCheckIsTextEditor() || view.canScrollVertically(-1)
            || view.canScrollVertically(1) || view.canScrollHorizontally(-1)
            || view.canScrollHorizontally(1)) return true;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = group.getChildCount() - 1; i >= 0; i--) {
                if (keyboardDragBlocked(group.getChildAt(i), x, y)) return true;
            }
        }
        return false;
    }

    private void updateInsets() {
        windowBounds = getWindowManager().getCurrentWindowMetrics().getBounds();
        WindowInsets value = imeAnimations.isEmpty() ?
            getWindow().getDecorView().getRootWindowInsets() : animatedInsets;
        if (value != null) updateInsets(value);
    }

    private void updateInsets(WindowInsets value) {
        // NativeActivity takes the window surface; it does not create a child SurfaceView.
        View surface = getWindow().getDecorView();
        int[] position = new int[2];
        surface.getLocationInWindow(position);
        float scale = getResources().getDisplayMetrics().density;
        android.graphics.Insets bars = value.getInsets(
            WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
        int top = bars.top, left = bars.left, bottom = bars.bottom, right = bars.right;
        int ime = value.getInsets(WindowInsets.Type.ime()).bottom;
        int below = windowBounds.height() - position[1] - surface.getHeight();
        int beside = windowBounds.width() - position[0] - surface.getWidth();
        float[] previousInsets = insets;
        float previousKeyboardInset = keyboardInset;
        boolean previousKeyboardVisible = keyboardVisible;
        insets = new float[] {
            Math.max(0, top - position[1]) / scale, Math.max(0, left - position[0]) / scale,
            Math.max(0, bottom - below) / scale, Math.max(0, right - beside) / scale
        };
        keyboardInset = Math.min(surface.getHeight(), Math.max(0, ime - below)) / scale;
        keyboardVisible = value.isVisible(WindowInsets.Type.ime()) || ime > 0;
        if (!keyboardVisible) keyboardDragging = false;
        synchronized (this) {
            if (!observingEvents || events == 0) return;
            if (!java.util.Arrays.equals(previousInsets, insets) || previousKeyboardInset != keyboardInset
                || previousKeyboardVisible != keyboardVisible) windowChanged();
        }
    }

    public float[] safeArea() {return insets.clone();}
    public float keyboardInset() {return keyboardInset;}
    public boolean keyboardVisible() {return keyboardVisible;}
    public void haptic() {
        runOnUiThread(() -> getWindow().getDecorView()
            .performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY));
    }

    public void clipboardWrite(String mime, String value) {
        try {
            ClipData clip = mime.equals("text/plain") ? ClipData.newPlainText("", value) :
                ClipData.newUri(getContentResolver(), "", SharedFileProvider.prepare(this, value));
            runOnUiThread(() -> {
                try {getSystemService(ClipboardManager.class).setPrimaryClip(clip);}
                catch (SecurityException error) {Log.w("Maverick", "Clipboard unavailable", error);}
            });
        } catch (Exception error) {Log.w("Maverick", "Clipboard write failed", error);}
    }

    public String clipboardRead() {
        try {
            ClipData clip = getSystemService(ClipboardManager.class).getPrimaryClip();
            if (clip == null || clip.getItemCount() != 1) return null;
            ClipData.Item item = clip.getItemAt(0);
            if (item.getUri() != null) return importUri(item.getUri(), false).toString();
            CharSequence text = item.getText();
            return text == null ? null : new JSONObject().put("text", text.toString()).toString();
        } catch (Exception error) {Log.w("Maverick", "Clipboard read failed", error); return null;}
    }

    public void share(String mime, String value) {
        try {
            Intent send = new Intent(Intent.ACTION_SEND).setType(mime);
            if (mime.equals("text/plain")) send.putExtra(Intent.EXTRA_TEXT, value);
            else {
                Uri uri = SharedFileProvider.prepare(this, value);
                send.setType(getContentResolver().getType(uri));
                send.putExtra(Intent.EXTRA_STREAM, uri);
                send.setClipData(ClipData.newRawUri("", uri));
                send.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
            }
            runOnUiThread(() -> {
                try {startActivity(Intent.createChooser(send, "Share"));}
                catch (android.content.ActivityNotFoundException error) {
                    Log.w("Maverick", "No sharing application available", error);
                }
            });
        } catch (Exception error) {Log.w("Maverick", "Sharing failed", error);}
    }

    public void notifyUser(String title, String body) {
        runOnUiThread(() -> {
            if (resumed) {
                synchronized (this) {
                    if (observingEvents && events != 0) notificationReceived(title, body);
                    else pendingNotifications.add(new String[] {title, body});
                }
                return;
            }
            if (android.os.Build.VERSION.SDK_INT >= 33 &&
                checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                notifications.add(new String[] {title, body});
                requestNotificationPermission();
                return;
            }
            MaverickNotifications.post(this, title, body, null);
        });
    }

    @Override public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        if (request == 60002) {if (capture != null) capture.permissions(); return;}
        if (request != 60001) return;
        if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) {
            for (String[] notification : notifications) {
                MaverickNotifications.post(this, notification[0], notification[1], null);
            }
        }
        notifications.clear();
    }

    public long beginPick(String mime, boolean multiple, boolean rawFiles, String title) throws java.io.IOException {
        int id = nextPick.getAndIncrement();
        if (id >= 60000) throw new IllegalStateException("Picker request identifiers exhausted");
        ParcelFileDescriptor[] pair = ParcelFileDescriptor.createSocketPair();
        int fd = pair[0].detachFd();
        runOnUiThread(() -> {
            picks.put(id, new PendingPick(pair[1], rawFiles));
            String[] types = mime.split(",");
            Intent pick = new Intent(Intent.ACTION_OPEN_DOCUMENT)
                .setType(types.length == 1 ? types[0] : "*/*")
                .addCategory(Intent.CATEGORY_OPENABLE).putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
            if (types.length > 1) pick.putExtra(Intent.EXTRA_MIME_TYPES, types);
            try {startActivityForResult(Intent.createChooser(pick, title), id);}
            catch (android.content.ActivityNotFoundException error) {cancelPick(id);}
        });
        return ((long) id << 32) | (fd & 0xffffffffL);
    }

    public void cancelPick(int id) {
        runOnUiThread(() -> {
            PendingPick result = picks.remove(id);
            if (result == null) return;
            finishActivity(id);
            try {result.output.close();} catch (java.io.IOException error) {Log.w("Maverick", "Picker close", error);}
        });
    }

    @Override protected void onActivityResult(int id, int result, Intent data) {
        super.onActivityResult(id, result, data);
        PendingPick pick = picks.remove(id);
        if (pick == null) return;
        new Thread(() -> {
            try (FileOutputStream stream = new ParcelFileDescriptor.AutoCloseOutputStream(pick.output)) {
                JSONObject response = new JSONObject();
                if (result != RESULT_OK || data == null) response.put("cancelled", true);
                else {
                    JSONArray items = new JSONArray();
                    ClipData clips = data.getClipData();
                    if (clips != null) {
                        for (int i = 0; i < clips.getItemCount(); i++) {
                            items.put(importUri(clips.getItemAt(i).getUri(), pick.rawFiles));
                        }
                    } else if (data.getData() != null) items.put(importUri(data.getData(), pick.rawFiles));
                    if (items.length() == 0) response.put("cancelled", true);
                    else response.put("items", items);
                }
                stream.write(response.toString().getBytes(StandardCharsets.UTF_8));
            } catch (Exception error) {Log.w("Maverick", "Picker result closed", error);}
        }, "Maverick picker").start();
    }

    private JSONObject importUri(Uri uri, boolean rawFiles) throws org.json.JSONException {
        File target = null;
        try {
            String mime = getContentResolver().getType(uri);
            boolean image = mime != null && mime.startsWith("image/");
            File directory = new File(getFilesDir(), "imports");
            if (!directory.isDirectory() && !directory.mkdirs()) throw new java.io.IOException("Cannot create imports");
            String name = "file";
            try (android.database.Cursor cursor = getContentResolver().query(
                    uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
                if (cursor != null && cursor.moveToFirst()) name = new File(cursor.getString(0)).getName();
            }
            target = new File(directory, UUID.randomUUID() + (image && !rawFiles ? ".png" : "-" + name));
            if (image && !rawFiles) {
                Bitmap bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(getContentResolver(), uri),
                    (decoder, info, source) -> decoder.setAllocator(ImageDecoder.ALLOCATOR_SOFTWARE));
                try (FileOutputStream output = new FileOutputStream(target)) {
                    if (!bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)) {
                        throw new java.io.IOException("Could not decode selected image");
                    }
                } finally {bitmap.recycle();}
            } else {
                try (InputStream input = getContentResolver().openInputStream(uri)) {
                    if (input == null) throw new java.io.IOException("Provider returned no content");
                    Files.copy(input, target.toPath());
                }
            }
            return new JSONObject().put("path", target.getAbsolutePath()).put("image", image)
                .put("media", mime != null && (mime.startsWith("audio/") || mime.startsWith("video/")));
        } catch (Exception error) {
            if (target != null) target.delete();
            return new JSONObject().put("error", error.toString());
        }
    }

    private static final class PendingPick {
        final ParcelFileDescriptor output;
        final boolean rawFiles;
        PendingPick(ParcelFileDescriptor output, boolean rawFiles) {
            this.output = output;
            this.rawFiles = rawFiles;
        }
    }

    @Override protected void onDestroy() {
        stopEvents();
        for (Integer id : new ArrayList<>(picks.keySet())) cancelPick(id);
        notifications.clear();
        pendingNotifications.clear();
        push.close();
        if (capture != null) capture.close();
        super.onDestroy();
        if (current.get() == this) current.clear();
    }
}
