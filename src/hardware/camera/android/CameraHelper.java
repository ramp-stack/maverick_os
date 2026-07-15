//NOTE
//if you are changing the .java source file, ensure you delete the stale .class and .dex files prior to recompiling and embedding

//compile with java
// javac --release 8 -cp /absolute/path/to/android.jar CameraHelper.java   

//embed .dex
// /absolute/path/to/Android/sdk/build-tools/d8 --lib /absolute/path/to/android.jar CameraHelper*.class --output .

package com.maverick.camera;

import android.Manifest;
import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import android.graphics.ImageFormat;
import android.hardware.camera2.CameraAccessException;
import android.hardware.camera2.CameraCaptureSession;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraDevice;
import android.hardware.camera2.CameraManager;
import android.hardware.camera2.CaptureRequest;
import android.hardware.camera2.params.StreamConfigurationMap;
import android.media.Image;
import android.media.ImageReader;
import android.os.Handler;
import android.os.HandlerThread;
import android.util.Size;
import android.view.Surface;

import java.util.Collections;

public class CameraHelper {

    private static final int REQUEST_CODE_CAMERA_PERMISSION = 2001;

    private final Context context;
    private final CameraManager cameraManager;

    private HandlerThread backgroundThread;
    private Handler backgroundHandler;

    private CameraDevice cameraDevice;
    private CameraCaptureSession captureSession;
    private ImageReader imageReader;

    private long framePtr;
    private int sensorOrientation;
    private volatile boolean closed = true;

    public CameraHelper(Context context) {
        this.context = context;
        this.cameraManager = (CameraManager) context.getSystemService(Context.CAMERA_SERVICE);
    }

    public boolean hasCameraPermission() {
        return context.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED;
    }

    public void requestCameraPermission() {
        if (context instanceof Activity) {
            ((Activity) context).requestPermissions(
                new String[]{ Manifest.permission.CAMERA },
                REQUEST_CODE_CAMERA_PERMISSION
            );
        }
    }

    public String[] getCameraIdList() {
        try {
            return cameraManager.getCameraIdList();
        } catch (CameraAccessException e) {
            return new String[0];
        }
    }

    public void openCamera(String cameraId, long framePtr) {
        if (!hasCameraPermission()) {
            return;
        }

        this.framePtr = framePtr;
        this.closed = false;

        try {
            CameraCharacteristics characteristics = cameraManager.getCameraCharacteristics(cameraId);
            Integer orientation = characteristics.get(CameraCharacteristics.SENSOR_ORIENTATION);
            sensorOrientation = orientation != null ? orientation : 0;
            Size size = chooseStreamSize(characteristics);

            imageReader = ImageReader.newInstance(
                size.getWidth(), size.getHeight(), ImageFormat.YUV_420_888, 2
            );

            startBackgroundThread();

            imageReader.setOnImageAvailableListener(new ImageAvailableListener(), backgroundHandler);

            cameraManager.openCamera(cameraId, new CameraStateCallback(), backgroundHandler);
        } catch (CameraAccessException | SecurityException e) {
            // no-op: camera simply never starts producing frames
        }
    }

    private Size chooseStreamSize(CameraCharacteristics characteristics) {
        StreamConfigurationMap map =
            characteristics.get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);
        Size[] sizes = map.getOutputSizes(ImageFormat.YUV_420_888);

        // Largest size with width <= 1280, so preview frames stay cheap to convert/render.
        Size chosen = null;
        for (Size candidate : sizes) {
            if (candidate.getWidth() > 1280) {
                continue;
            }
            if (chosen == null || (long) candidate.getWidth() * candidate.getHeight()
                    > (long) chosen.getWidth() * chosen.getHeight()) {
                chosen = candidate;
            }
        }

        if (chosen == null) {
            // No size <= 1280 wide is offered; fall back to the smallest available
            // rather than silently defaulting to the largest (full sensor res).
            chosen = sizes[0];
            for (Size candidate : sizes) {
                if ((long) candidate.getWidth() * candidate.getHeight()
                        < (long) chosen.getWidth() * chosen.getHeight()) {
                    chosen = candidate;
                }
            }
        }

        return chosen;
    }

    private void startCaptureSession() {
        if (cameraDevice == null || imageReader == null) {
            return;
        }

        try {
            Surface surface = imageReader.getSurface();
            CaptureRequest.Builder requestBuilder =
                cameraDevice.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
            requestBuilder.addTarget(surface);

            cameraDevice.createCaptureSession(
                Collections.singletonList(surface),
                new SessionStateCallback(requestBuilder),
                backgroundHandler
            );
        } catch (CameraAccessException e) {
            // no-op: session simply never gets created
        }
    }

    class CameraStateCallback extends CameraDevice.StateCallback {
        @Override
        public void onOpened(CameraDevice device) {
            if (closed) {
                // closeCamera() ran before this async callback fired.
                device.close();
                return;
            }
            cameraDevice = device;
            startCaptureSession();
        }

        @Override
        public void onDisconnected(CameraDevice device) {
            device.close();
            cameraDevice = null;
        }

        @Override
        public void onError(CameraDevice device, int error) {
            device.close();
            cameraDevice = null;
        }
    }


    class SessionStateCallback extends CameraCaptureSession.StateCallback {
        private final CaptureRequest.Builder requestBuilder;

        SessionStateCallback(CaptureRequest.Builder requestBuilder) {
            this.requestBuilder = requestBuilder;
        }

        @Override
        public void onConfigured(CameraCaptureSession session) {
            if (closed) {
                // closeCamera() ran before this async callback fired.
                session.close();
                return;
            }
            captureSession = session;
            try {
                session.setRepeatingRequest(requestBuilder.build(), null, backgroundHandler);
            } catch (CameraAccessException | IllegalStateException e) {
                // no-op: session simply never produces frames
            }
        }

        @Override
        public void onConfigureFailed(CameraCaptureSession session) {
        }
    }

    // Runs on backgroundHandler's thread (Camera2 always delivers ImageReader
    // callbacks there), so the native conversion work never touches the app's
    // main/render thread.
    class ImageAvailableListener implements ImageReader.OnImageAvailableListener {
        @Override
        public void onImageAvailable(ImageReader reader) {
            Image image = reader.acquireLatestImage();
            if (image == null) {
                return;
            }
            nativeOnFrameAvailable(framePtr, image, sensorOrientation);
            image.close();
        }
    }

    private static native void nativeOnFrameAvailable(long framePtr, Image image, int rotationDegrees);

    public void closeCamera() {
        closed = true;

        if (captureSession != null) {
            captureSession.close();
            captureSession = null;
        }
        if (cameraDevice != null) {
            cameraDevice.close();
            cameraDevice = null;
        }
        if (imageReader != null) {
            imageReader.close();
            imageReader = null;
        }

        stopBackgroundThread();
    }

    private void startBackgroundThread() {
        if (backgroundThread != null) {
            return;
        }
        backgroundThread = new HandlerThread("CameraHelperBackground");
        backgroundThread.start();
        backgroundHandler = new Handler(backgroundThread.getLooper());
    }

    private void stopBackgroundThread() {
        if (backgroundThread == null) {
            return;
        }
        backgroundThread.quitSafely();
        try {
            backgroundThread.join();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
        }
        backgroundThread = null;
        backgroundHandler = null;
    }
}
