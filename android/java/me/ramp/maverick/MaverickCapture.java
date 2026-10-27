package me.ramp.maverick;

import android.Manifest;
import android.content.pm.PackageManager;
import android.graphics.ImageFormat;
import android.hardware.camera2.CameraCaptureSession;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraDevice;
import android.hardware.camera2.CameraManager;
import android.hardware.camera2.CaptureRequest;
import android.hardware.camera2.CaptureResult;
import android.hardware.camera2.TotalCaptureResult;
import android.media.AudioFormat;
import android.media.AudioRecord;
import android.media.AudioTimestamp;
import android.media.Image;
import android.media.ImageReader;
import android.media.MediaRecorder;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.SystemClock;
import android.util.Size;
import java.nio.ByteBuffer;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Comparator;

final class MaverickCapture {
    private final MaverickActivity activity;
    private final HandlerThread thread = new HandlerThread("maverick.camera");
    private final Handler handler;
    private long nativeState;
    private boolean closed;
    private boolean wantCamera;
    private boolean wantMicrophone;
    private String[] requestedPermissions;
    private volatile int generation;
    private CameraDevice camera;
    private ImageReader images;
    private CameraCaptureSession session;
    private AudioRecord microphone;
    private Thread audioThread;
    private long videoOffset;
    private volatile long videoDuration;

    private native void video(ByteBuffer y, ByteBuffer u, ByteBuffer v, int yStride,
        int uvStride, int uvPixelStride, int width, int height, long time, long duration, int rotation);
    private native void audio(float[] samples, int length, int rate, long time);
    private native void failed(boolean camera, String error);

    MaverickCapture(MaverickActivity activity) {
        this.activity = activity;
        thread.start();
        handler = new Handler(thread.getLooper());
    }

    void configure(boolean video, boolean audio) {
        activity.runOnUiThread(() -> {
            if (closed) return;
            wantCamera = video;
            wantMicrophone = audio;
            update();
        });
    }

    void permissions() {
        String[] requested = requestedPermissions;
        requestedPermissions = null;
        if (closed || requested == null) return;
        for (String permission : requested) {
            if (activity.checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED) continue;
            if (permission.equals(Manifest.permission.CAMERA)) {
                if (wantCamera) error(true, "Camera access denied; allow it in system settings");
                wantCamera = false;
            } else if (permission.equals(Manifest.permission.RECORD_AUDIO)) {
                if (wantMicrophone) error(false, "Microphone access denied; allow it in system settings");
                wantMicrophone = false;
            }
        }
        update();
    }

    private void update() {
        if (!wantCamera) stopCamera();
        if (!wantMicrophone) stopMicrophone();
        ArrayList<String> permissions = new ArrayList<>();
        if (wantCamera && activity.checkSelfPermission(Manifest.permission.CAMERA)
            != PackageManager.PERMISSION_GRANTED) permissions.add(Manifest.permission.CAMERA);
        if (wantMicrophone && activity.checkSelfPermission(Manifest.permission.RECORD_AUDIO)
            != PackageManager.PERMISSION_GRANTED) permissions.add(Manifest.permission.RECORD_AUDIO);
        if (!permissions.isEmpty()) {
            if (requestedPermissions == null) {
                requestedPermissions = permissions.toArray(new String[0]);
                activity.requestPermissions(requestedPermissions, 60002);
            }
            return;
        }
        if (wantCamera && images == null) startCamera();
        if (wantMicrophone && microphone == null) startMicrophone();
    }

    private void startCamera() {
        int operation = ++generation;
        try {
            CameraManager manager = activity.getSystemService(CameraManager.class);
            String[] cameras = manager.getCameraIdList();
            if (cameras.length == 0) throw new IllegalStateException("No camera is available");
            String selected = cameras[0];
            for (String id : cameras) {
                if (manager.getCameraCharacteristics(id).get(CameraCharacteristics.LENS_FACING)
                    == CameraCharacteristics.LENS_FACING_BACK) {selected = id; break;}
            }
            CameraCharacteristics info = manager.getCameraCharacteristics(selected);
            Size[] sizes = info.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                .getOutputSizes(ImageFormat.YUV_420_888);
            Size size = Arrays.stream(sizes).min(Comparator.comparingLong(value ->
                Math.abs((long)value.getWidth() * value.getHeight() - 1280 * 720))).get();
            int sensor = info.get(CameraCharacteristics.SENSOR_ORIENTATION);
            boolean front = info.get(CameraCharacteristics.LENS_FACING)
                == CameraCharacteristics.LENS_FACING_FRONT;
            boolean realtime = info.get(CameraCharacteristics.SENSOR_INFO_TIMESTAMP_SOURCE)
                == CameraCharacteristics.SENSOR_INFO_TIMESTAMP_SOURCE_REALTIME;
            long interval = info.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP)
                .getOutputMinFrameDuration(ImageFormat.YUV_420_888, size);
            videoOffset = Long.MIN_VALUE;
            videoDuration = interval > 0 ? interval : 33333333L;
            images = ImageReader.newInstance(size.getWidth(), size.getHeight(), ImageFormat.YUV_420_888, 3);
            ImageReader reader = images;
            reader.setOnImageAvailableListener(source -> {
                try (Image image = source.acquireLatestImage()) {
                    if (image == null) return;
                    synchronized (this) {
                        if (nativeState == 0 || operation != generation) return;
                        long timestamp = image.getTimestamp();
                        // UNKNOWN camera clocks preserve intervals but need an arrival-time anchor.
                        if (videoOffset == Long.MIN_VALUE) videoOffset = realtime ? 0 :
                            SystemClock.elapsedRealtimeNanos() - timestamp;
                        timestamp += videoOffset;
                        int display = activity.getDisplay().getRotation() * 90;
                        int rotation = (sensor + (front ? display : -display) + 360) % 360;
                        Image.Plane[] planes = image.getPlanes();
                        video(planes[0].getBuffer(), planes[1].getBuffer(), planes[2].getBuffer(),
                            planes[0].getRowStride(), planes[1].getRowStride(), planes[1].getPixelStride(),
                            image.getWidth(), image.getHeight(), timestamp, videoDuration, rotation);
                    }
                } catch (IllegalStateException error) {
                    if (operation == generation) error(true, error.toString());
                }
            }, handler);
            Handler callbacks = new Handler(activity.getMainLooper());
            manager.openCamera(selected, new CameraDevice.StateCallback() {
                @Override public void onOpened(CameraDevice device) {
                    if (closed || operation != generation) {device.close(); return;}
                    camera = device;
                    try {
                        device.createCaptureSession(Collections.singletonList(reader.getSurface()),
                            new CameraCaptureSession.StateCallback() {
                            @Override public void onConfigured(CameraCaptureSession configured) {
                                if (closed || operation != generation) {configured.close(); return;}
                                session = configured;
                                try {
                                    CaptureRequest.Builder request = device.createCaptureRequest(
                                        CameraDevice.TEMPLATE_RECORD);
                                    request.addTarget(reader.getSurface());
                                    configured.setRepeatingRequest(request.build(),
                                        new CameraCaptureSession.CaptureCallback() {
                                        @Override public void onCaptureCompleted(CameraCaptureSession owner,
                                            CaptureRequest request, TotalCaptureResult result) {
                                            Long duration = result.get(CaptureResult.SENSOR_FRAME_DURATION);
                                            if (duration != null && operation == generation) {
                                                videoDuration = duration;
                                            }
                                        }
                                    }, handler);
                                } catch (Exception error) {cameraError(operation, error.toString());}
                            }
                            @Override public void onConfigureFailed(CameraCaptureSession value) {
                                cameraError(operation, "Camera session could not be configured");
                            }
                        }, callbacks);
                    } catch (Exception error) {cameraError(operation, error.toString());}
                }
                @Override public void onDisconnected(CameraDevice device) {
                    device.close();
                    cameraError(operation, "Camera disconnected");
                }
                @Override public void onError(CameraDevice device, int code) {
                    device.close();
                    cameraError(operation, "Camera unavailable: " + code);
                }
            }, callbacks);
        } catch (Exception error) {cameraError(operation, error.toString());}
    }

    private void cameraError(int operation, String message) {
        activity.runOnUiThread(() -> {
            if (closed || operation != generation) return;
            error(true, message);
            stopCamera();
        });
    }

    private void startMicrophone() {
        try {
            AudioRecord record = new AudioRecord.Builder()
                .setAudioSource(MediaRecorder.AudioSource.DEFAULT)
                .setAudioFormat(new AudioFormat.Builder().setChannelMask(AudioFormat.CHANNEL_IN_MONO)
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT).build()).build();
            int rate = record.getSampleRate();
            microphone = record;
            record.startRecording();
            long origin = SystemClock.elapsedRealtimeNanos();
            audioThread = new Thread(() -> {
                float[] samples = new float[rate / 100];
                AudioTimestamp stamp = new AudioTimestamp();
                long position = 0;
                long end = origin;
                while (!Thread.currentThread().isInterrupted()) {
                    int count = record.read(samples, 0, samples.length, AudioRecord.READ_BLOCKING);
                    if (count < 0) {
                        synchronized (this) {
                            if (microphone == record) error(false, "Microphone read failed: " + count);
                        }
                        break;
                    }
                    if (count == 0) continue;
                    long time = origin + position * 1000000000L / rate;
                    if (record.getTimestamp(stamp, AudioTimestamp.TIMEBASE_BOOTTIME) == AudioRecord.SUCCESS) {
                        time = stamp.nanoTime + (position - stamp.framePosition) * 1000000000L / rate;
                    }
                    // The first hardware timestamp can correct the startup estimate backward.
                    time = Math.max(time, end);
                    end = time + count * 1000000000L / rate;
                    synchronized (this) {
                        if (nativeState == 0 || microphone != record) break;
                        audio(samples, count, rate, time);
                    }
                    position += count;
                }
                activity.runOnUiThread(() -> {if (microphone == record) stopMicrophone();});
            }, "maverick.microphone");
            audioThread.start();
        } catch (Exception error) {error(false, error.toString()); stopMicrophone();}
    }

    private synchronized void error(boolean camera, String message) {
        if (nativeState != 0) failed(camera, message);
    }

    private void stopCamera() {
        synchronized (this) {generation++;}
        if (session != null) {session.close(); session = null;}
        if (camera != null) {camera.close(); camera = null;}
        if (images != null) {images.close(); images = null;}
    }

    private void stopMicrophone() {
        AudioRecord record;
        synchronized (this) {record = microphone; microphone = null;}
        if (record == null) return;
        if (audioThread != null) audioThread.interrupt();
        if (record.getRecordingState() == AudioRecord.RECORDSTATE_RECORDING) record.stop();
        if (audioThread != null) {
            try {audioThread.join();} catch (InterruptedException error) {Thread.currentThread().interrupt();}
            audioThread = null;
        }
        record.release();
    }

    void close() {
        activity.runOnUiThread(() -> {
            if (closed) return;
            error(true, "Capture session closed");
            error(false, "Capture session closed");
            closed = true;
            stopCamera();
            stopMicrophone();
            thread.quitSafely();
        });
    }
}
