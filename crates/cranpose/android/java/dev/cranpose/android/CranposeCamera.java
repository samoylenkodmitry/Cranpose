package dev.cranpose.android;

import android.Manifest;
import android.app.Activity;
import android.content.Context;
import android.content.pm.PackageManager;
import android.graphics.ImageFormat;
import android.hardware.camera2.CameraCaptureSession;
import android.hardware.camera2.CameraCharacteristics;
import android.hardware.camera2.CameraDevice;
import android.hardware.camera2.CameraManager;
import android.hardware.camera2.CaptureRequest;
import android.hardware.camera2.params.OutputConfiguration;
import android.hardware.camera2.params.SessionConfiguration;
import android.hardware.camera2.params.StreamConfigurationMap;
import android.media.Image;
import android.media.ImageReader;
import android.os.Handler;
import android.os.HandlerThread;
import android.os.Build;
import android.util.Size;
import android.view.Surface;

import java.nio.ByteBuffer;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.atomic.AtomicLong;

/**
 * The Camera2 session behind Cranpose's camera service.
 *
 * <p>Frames go straight to native code as NV12 bytes — the format the sensor
 * already produces — through {@link CranposeActivity#onCameraFrame}. They used
 * to be JPEG-compressed, written to a file and read back, which cost an encode,
 * a file write, a rename, a read and a decode on every frame and capped the
 * preview at fifteen frames a second to afford it. Nothing here is polled and
 * nothing waits: a still is asked for and arrives, rather than being waited for
 * with a marker file and a sleep loop.
 */
final class CranposeCamera {
    private final Activity activity;
    /**
     * Whether a frame is still being delivered to native code.
     *
     * <p>A detector slower than the frame rate must fall behind by frames
     * rather than by memory: a queue of stale frames costs memory to hold and
     * answers about a scene that has already moved. Frames produced while one
     * is in flight are counted and dropped.
     */
    private final AtomicBoolean deliveringFrame = new AtomicBoolean(false);
    private final AtomicLong frameSequence = new AtomicLong(0);
    private CameraDevice camera;
    private CameraCaptureSession session;
    private ImageReader previewReader;
    private ImageReader stillReader;
    private HandlerThread cameraThread;
    private Handler cameraHandler;
    private HandlerThread previewThread;
    private Handler previewHandler;
    private volatile boolean open = false;
    private volatile String chosenId = null;
    private volatile String openId = null;
    private volatile int flash = 0;
    private volatile int rotationDegrees = 0;

    CranposeCamera(Activity activity) {
        this.activity = activity;
    }

    private CameraManager manager() {
        return (CameraManager) activity.getSystemService(Context.CAMERA_SERVICE);
    }

    private static float shortestFocalLength(CameraCharacteristics chars) {
        float[] lengths = chars.get(CameraCharacteristics.LENS_INFO_AVAILABLE_FOCAL_LENGTHS);
        if (lengths == null || lengths.length == 0) {
            return Float.MAX_VALUE;
        }
        float shortest = lengths[0];
        for (float length : lengths) {
            shortest = Math.min(shortest, length);
        }
        return shortest;
    }

    private static boolean takesPictures(CameraCharacteristics chars) {
        int[] caps = chars.get(CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES);
        if (caps == null) {
            return false;
        }
        for (int cap : caps) {
            if (cap == CameraCharacteristics.REQUEST_AVAILABLE_CAPABILITIES_BACKWARD_COMPATIBLE) {
                return true;
            }
        }
        return false;
    }

    private java.util.List<String> backIds() {
        java.util.List<String> ids = new java.util.ArrayList<>();
        try {
            for (String id : manager().getCameraIdList()) {
                CameraCharacteristics chars = manager().getCameraCharacteristics(id);
                Integer facing = chars.get(CameraCharacteristics.LENS_FACING);
                if (facing != null && facing == CameraCharacteristics.LENS_FACING_BACK
                        && takesPictures(chars)) {
                    ids.add(id);
                }
            }
            ids.sort((a, b) -> {
                try {
                    return Float.compare(
                            shortestFocalLength(manager().getCameraCharacteristics(a)),
                            shortestFocalLength(manager().getCameraCharacteristics(b)));
                } catch (Exception e) {
                    return 0;
                }
            });
        } catch (Exception e) {
            android.util.Log.w("cranpose", "camera list failed", e);
        }
        return ids;
    }

    String lensList() {
        java.util.List<String> ids = backIds();
        String[] wideNames = {"Ultra wide", "Wide", "Tele"};
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < ids.size(); i++) {
            String name;
            if (ids.size() == 1) {
                name = "Back";
            } else if (ids.size() <= wideNames.length) {
                name = wideNames[wideNames.length - ids.size() + i];
            } else if (i < wideNames.length) {
                name = wideNames[i];
            } else {
                name = "Lens " + (i + 1);
            }
            out.append(ids.get(i)).append('|').append(name).append('\n');
        }
        return out.toString();
    }

    int rotationDegrees() {
        return rotationDegrees;
    }

    @SuppressWarnings("deprecation")
    private int displayRotationDegrees() {
        android.view.Display display = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R
                ? activity.getDisplay()
                : activity.getWindowManager().getDefaultDisplay();
        if (display == null) {
            return 0;
        }
        switch (display.getRotation()) {
            case Surface.ROTATION_90:
                return 90;
            case Surface.ROTATION_180:
                return 180;
            case Surface.ROTATION_270:
                return 270;
            default:
                return 0;
        }
    }

    String currentLens() {
        String id = openId != null ? openId : chosenId;
        if (id != null) {
            return id;
        }
        java.util.List<String> ids = backIds();
        if (ids.isEmpty()) {
            return "";
        }
        return ids.get(ids.size() > 1 ? 1 : 0);
    }

    synchronized void useLens(String id) {
        if (id == null || id.isEmpty() || id.equals(openId)) {
            return;
        }
        chosenId = id;
        boolean wasOpen = open;
        stop();
        if (wasOpen) {
            start();
        }
    }

    boolean hasFlash() {
        try {
            String id = currentLens();
            if (id.isEmpty()) {
                java.util.List<String> ids = backIds();
                if (ids.isEmpty()) {
                    return false;
                }
                id = ids.get(ids.size() > 1 ? 1 : 0);
            }
            Boolean available = manager().getCameraCharacteristics(id)
                    .get(CameraCharacteristics.FLASH_INFO_AVAILABLE);
            return available != null && available;
        } catch (Exception e) {
            return false;
        }
    }

    synchronized void setFlash(int mode) {
        flash = mode;
        if (session == null || camera == null) {
            return;
        }
        try {
            CaptureRequest.Builder request =
                    camera.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
            request.addTarget(previewReader.getSurface());
            request.set(CaptureRequest.CONTROL_AF_MODE,
                    CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE);
            applyFlash(request, false);
            session.setRepeatingRequest(request.build(), null, cameraHandler);
        } catch (Exception e) {
            android.util.Log.w("cranpose", "flash request failed", e);
        }
    }

    private void applyFlash(CaptureRequest.Builder request, boolean still) {
        if (flash == 2) {
            request.set(CaptureRequest.CONTROL_AE_MODE,
                    still ? CaptureRequest.CONTROL_AE_MODE_ON_ALWAYS_FLASH
                            : CaptureRequest.CONTROL_AE_MODE_ON);
            request.set(CaptureRequest.FLASH_MODE,
                    still ? CaptureRequest.FLASH_MODE_SINGLE : CaptureRequest.FLASH_MODE_TORCH);
        } else if (flash == 1) {
            request.set(CaptureRequest.CONTROL_AE_MODE,
                    CaptureRequest.CONTROL_AE_MODE_ON_AUTO_FLASH);
        } else {
            request.set(CaptureRequest.CONTROL_AE_MODE, CaptureRequest.CONTROL_AE_MODE_ON);
            request.set(CaptureRequest.FLASH_MODE, CaptureRequest.FLASH_MODE_OFF);
        }
    }

    boolean hasPermission() {
        return activity.checkSelfPermission(Manifest.permission.CAMERA)
                == PackageManager.PERMISSION_GRANTED;
    }

    synchronized void start() {
        if (open || !hasPermission()) {
            return;
        }
        open = true;
        frameSequence.set(0);
        deliveringFrame.set(false);
        cameraThread = new HandlerThread("cranpose-camera-control");
        cameraThread.start();
        cameraHandler = new Handler(cameraThread.getLooper());
        previewThread = new HandlerThread("cranpose-camera-frames");
        previewThread.start();
        previewHandler = new Handler(previewThread.getLooper());
        try {
            CameraManager manager = manager();
            java.util.List<String> ids = backIds();
            String backId = null;
            if (chosenId != null && ids.contains(chosenId)) {
                backId = chosenId;
            } else if (ids.size() > 1) {
                backId = ids.get(1);
            } else if (ids.size() == 1) {
                backId = ids.get(0);
            }
            if (backId == null) {
                for (String id : manager.getCameraIdList()) {
                    Integer facing = manager.getCameraCharacteristics(id)
                            .get(CameraCharacteristics.LENS_FACING);
                    if (facing != null && facing == CameraCharacteristics.LENS_FACING_BACK) {
                        backId = id;
                        break;
                    }
                }
            }
            if (backId == null) {
                CranposeActivity.onCameraFailed("this device exposes no usable camera");
                stop();
                return;
            }
            openId = backId;
            CameraCharacteristics chars = manager.getCameraCharacteristics(backId);
            Integer sensorOrientation = chars.get(CameraCharacteristics.SENSOR_ORIENTATION);
            rotationDegrees = ((sensorOrientation == null ? 0 : sensorOrientation)
                    - displayRotationDegrees() + 360) % 360;
            StreamConfigurationMap streamMap = chars.get(
                    CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP);
            if (streamMap == null) {
                throw new IllegalStateException("camera has no stream configuration map");
            }
            Size[] jpegSizes = streamMap.getOutputSizes(ImageFormat.JPEG);
            Size still = Arrays.stream(jpegSizes)
                    .max((a, b) -> a.getWidth() * a.getHeight() - b.getWidth() * b.getHeight())
                    .orElse(jpegSizes[0]);
            Size preview = choosePreviewSize(streamMap.getOutputSizes(ImageFormat.YUV_420_888));

            previewReader = ImageReader.newInstance(
                    preview.getWidth(), preview.getHeight(), ImageFormat.YUV_420_888, 2);
            previewReader.setOnImageAvailableListener(this::onPreviewFrame, previewHandler);
            stillReader = ImageReader.newInstance(
                    still.getWidth(), still.getHeight(), ImageFormat.JPEG, 1);
            stillReader.setOnImageAvailableListener(this::onStill, previewHandler);

            manager.openCamera(backId, new CameraDevice.StateCallback() {
                @Override
                public void onOpened(CameraDevice device) {
                    camera = device;
                    createSession();
                }

                @Override
                public void onDisconnected(CameraDevice device) {
                    stop();
                }

                @Override
                public void onError(CameraDevice device, int error) {
                    CranposeActivity.onCameraFailed("the camera reported error " + error);
                    stop();
                }
            }, cameraHandler);
        } catch (Exception error) {
            CranposeActivity.onCameraFailed(String.valueOf(error.getMessage()));
            stop();
        }
    }

    @SuppressWarnings("deprecation")
    private void createSession() {
        try {
            CameraCaptureSession.StateCallback callback = new CameraCaptureSession.StateCallback() {
                @Override
                public void onConfigured(CameraCaptureSession s) {
                    session = s;
                    try {
                        CaptureRequest.Builder request = camera
                                .createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW);
                        request.addTarget(previewReader.getSurface());
                        request.set(CaptureRequest.CONTROL_AF_MODE,
                                CaptureRequest.CONTROL_AF_MODE_CONTINUOUS_PICTURE);
                        applyFlash(request, false);
                        s.setRepeatingRequest(request.build(), null, cameraHandler);
                        CranposeActivity.onCameraRunning(openId == null ? "" : openId);
                    } catch (Exception error) {
                        CranposeActivity.onCameraFailed(String.valueOf(error.getMessage()));
                        stop();
                    }
                }

                @Override
                public void onConfigureFailed(CameraCaptureSession s) {
                    CranposeActivity.onCameraFailed("the camera session could not be configured");
                    stop();
                }
            };
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                SessionConfiguration configuration = new SessionConfiguration(
                        SessionConfiguration.SESSION_REGULAR,
                        Arrays.asList(
                                new OutputConfiguration(previewReader.getSurface()),
                                new OutputConfiguration(stillReader.getSurface())),
                        command -> cameraHandler.post(command),
                        callback);
                camera.createCaptureSession(configuration);
            } else {
                camera.createCaptureSession(
                        Arrays.asList(previewReader.getSurface(), stillReader.getSurface()),
                        callback,
                        cameraHandler);
            }
        } catch (Exception error) {
            CranposeActivity.onCameraFailed(String.valueOf(error.getMessage()));
            stop();
        }
    }

    private void onPreviewFrame(ImageReader reader) {
        try (Image image = reader.acquireLatestImage()) {
            if (image == null || !open) {
                return;
            }
            if (!deliveringFrame.compareAndSet(false, true)) {
                CranposeActivity.onCameraFrameDropped();
                return;
            }
            try {
                byte[] nv12 = yuv420ToNv12(image);
                CranposeActivity.onCameraFrame(
                        nv12,
                        image.getWidth(),
                        image.getHeight(),
                        rotationDegrees,
                        frameSequence.incrementAndGet());
            } finally {
                deliveringFrame.set(false);
            }
        } catch (Exception error) {
            android.util.Log.w("cranpose", "preview frame failed", error);
        }
    }

    private void onStill(ImageReader reader) {
        try (Image image = reader.acquireLatestImage()) {
            if (image == null) {
                CranposeActivity.onCameraStill(null, "the still capture produced no image");
                return;
            }
            ByteBuffer buffer = image.getPlanes()[0].getBuffer();
            byte[] bytes = new byte[buffer.remaining()];
            buffer.get(bytes);
            CranposeActivity.onCameraStill(bytes, "");
        } catch (Exception error) {
            CranposeActivity.onCameraStill(null, String.valueOf(error.getMessage()));
        }
    }

    synchronized void takeStill() {
        if (session == null || camera == null) {
            CranposeActivity.onCameraStill(null, "the camera is not running");
            return;
        }
        try {
            CaptureRequest.Builder request =
                    camera.createCaptureRequest(CameraDevice.TEMPLATE_STILL_CAPTURE);
            request.addTarget(stillReader.getSurface());
            request.set(CaptureRequest.JPEG_ORIENTATION, rotationDegrees);
            applyFlash(request, true);
            session.capture(request.build(), null, cameraHandler);
        } catch (Exception error) {
            CranposeActivity.onCameraStill(null, String.valueOf(error.getMessage()));
        }
    }

    synchronized void stop() {
        open = false;
        openId = null;
        try {
            if (session != null) {
                session.close();
                session = null;
            }
            if (camera != null) {
                camera.close();
                camera = null;
            }
            if (previewReader != null) {
                previewReader.close();
                previewReader = null;
            }
            if (stillReader != null) {
                stillReader.close();
                stillReader = null;
            }
            if (cameraThread != null) {
                cameraThread.quitSafely();
                cameraThread = null;
                cameraHandler = null;
            }
            if (previewThread != null) {
                previewThread.quitSafely();
                previewThread = null;
                previewHandler = null;
            }
        } catch (Exception ignored) {
        }
        deliveringFrame.set(false);
        CranposeActivity.onCameraStopped();
    }

    private static Size choosePreviewSize(Size[] sizes) {
        if (sizes == null || sizes.length == 0) {
            throw new IllegalArgumentException("camera exposes no YUV preview sizes");
        }

        final long targetArea = 640L * 480L;
        return Arrays.stream(sizes)
                .filter(size -> size.getWidth() >= 640 && size.getHeight() >= 480)
                .min((a, b) -> Long.compare(previewSizeScore(a, targetArea),
                        previewSizeScore(b, targetArea)))
                .orElseGet(() -> Arrays.stream(sizes)
                        .min((a, b) -> Long.compare(
                                Math.abs((long) a.getWidth() * a.getHeight() - targetArea),
                                Math.abs((long) b.getWidth() * b.getHeight() - targetArea)))
                        .orElse(sizes[0]));
    }

    private static long previewSizeScore(Size size, long targetArea) {
        long area = (long) size.getWidth() * size.getHeight();
        long areaDistance = Math.abs(area - targetArea);
        long aspectPenalty = Math.abs((long) size.getWidth() * 3L - (long) size.getHeight() * 4L);
        return areaDistance + aspectPenalty * 2_000L;
    }



    /**
     * Packs a {@code YUV_420_888} image into NV12: the luma plane followed by
     * an interleaved half-resolution U,V plane.
     *
     * <p>NV12 rather than NV21 — U before V — because that is what the
     * framework's {@code FrameFormat::Nv12} converts, and one order agreed on
     * both sides of the boundary beats two that have to be kept in step.
     */
    private static byte[] yuv420ToNv12(Image image) {
        int width = image.getWidth();
        int height = image.getHeight();
        byte[] nv12 = new byte[width * height * 3 / 2];
        Image.Plane y = image.getPlanes()[0];
        Image.Plane u = image.getPlanes()[1];
        Image.Plane v = image.getPlanes()[2];
        ByteBuffer yBuf = y.getBuffer();
        int pos = 0;
        int yRowStride = y.getRowStride();
        for (int row = 0; row < height; row++) {
            yBuf.position(row * yRowStride);
            yBuf.get(nv12, pos, width);
            pos += width;
        }
        ByteBuffer uBuf = u.getBuffer();
        ByteBuffer vBuf = v.getBuffer();
        int uvRowStride = u.getRowStride();
        int uvPixelStride = u.getPixelStride();
        for (int row = 0; row < height / 2; row++) {
            for (int col = 0; col < width / 2; col++) {
                int uvIndex = row * uvRowStride + col * uvPixelStride;
                nv12[pos++] = uBuf.get(uvIndex);
                nv12[pos++] = vBuf.get(uvIndex);
            }
        }
        return nv12;
    }
}
