package dev.cranpose.android;

import android.media.MediaCodecInfo;
import android.media.MediaCodecList;
import android.os.Build;

/**
 * What the device's video decoders take, for {@code VideoDecoder::capabilities}.
 *
 * <p>The NDK has no media codec list at the API levels that headsets run, so the limits come from
 * {@link MediaCodecInfo.VideoCapabilities}. Native code calls these methods from any thread. Each
 * call reads the codec list again, so an app asks once for each codec.
 */
final class CranposeVideoCodecs {
    private CranposeVideoCodecs() {}

    /**
     * The decoder that {@code MediaCodec.createDecoderByType(mime)} opens: the first decoder of the
     * codec list that supports the type, which is how the framework picks it.
     */
    private static MediaCodecInfo decoder(String mime) {
        for (MediaCodecInfo info : new MediaCodecList(MediaCodecList.REGULAR_CODECS).getCodecInfos()) {
            if (info.isEncoder()) {
                continue;
            }
            for (String type : info.getSupportedTypes()) {
                if (type.equalsIgnoreCase(mime)) {
                    return info;
                }
            }
        }
        return null;
    }

    private static MediaCodecInfo.VideoCapabilities video(MediaCodecInfo info, String mime) {
        return info == null ? null : info.getCapabilitiesForType(mime).getVideoCapabilities();
    }

    /** The name of the decoder for {@code mime}, or null when the device has none. */
    static String decoderName(String mime) {
        MediaCodecInfo info = decoder(mime);
        return info == null ? null : info.getName();
    }

    /**
     * The limits of the decoder for {@code mime}: the widest and the tallest picture, the highest
     * bit rate, and 1, 0 or -1 for hardware acceleration, true, false or unknown (before Android
     * 10). Null when the device has no video decoder for the type.
     */
    static int[] limits(String mime) {
        MediaCodecInfo info = decoder(mime);
        MediaCodecInfo.VideoCapabilities video = video(info, mime);
        if (video == null) {
            return null;
        }
        int hardware = Build.VERSION.SDK_INT >= 29 ? (info.isHardwareAccelerated() ? 1 : 0) : -1;
        return new int[] {
            video.getSupportedWidths().getUpper(),
            video.getSupportedHeights().getUpper(),
            video.getBitrateRange().getUpper(),
            hardware,
        };
    }

    /**
     * The highest frame rate at which the decoder for {@code mime} takes pictures of {@code width}
     * by {@code height} pixels, or 0 when it takes none of that size.
     */
    static double maxFrameRate(String mime, int width, int height) {
        MediaCodecInfo.VideoCapabilities video = video(decoder(mime), mime);
        if (video == null || !video.isSizeSupported(width, height)) {
            return 0;
        }
        try {
            return video.getSupportedFrameRatesFor(width, height).getUpper();
        } catch (IllegalArgumentException unsupported) {
            return 0;
        }
    }
}
