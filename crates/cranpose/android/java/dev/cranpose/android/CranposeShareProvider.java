package dev.cranpose.android;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;

import java.io.File;
import java.io.FileNotFoundException;

/**
 * A minimal read-only {@link ContentProvider} serving the files that
 * {@link CranposeActivity#cranposeShare} stages under
 * {@code cacheDir/cranpose_share/}, so the system share sheet can hand them to
 * other apps without a {@code file://} URI (forbidden since Android 7) and
 * without an androidx FileProvider dependency.
 *
 * <p>Apps declare it once in their manifest, with the authority
 * {@code <applicationId>.cranpose.share}:
 *
 * <pre>{@code
 * <provider
 *     android:name="dev.cranpose.android.CranposeShareProvider"
 *     android:authorities="${applicationId}.cranpose.share"
 *     android:exported="false"
 *     android:grantUriPermissions="true" />
 * }</pre>
 */
public class CranposeShareProvider extends ContentProvider {
    /** The staging directory under the app's cache dir. */
    static final String SHARE_DIR = "cranpose_share";

    static Uri uriFor(Context context, File staged) {
        return new Uri.Builder()
                .scheme("content")
                .authority(context.getPackageName() + ".cranpose.share")
                .appendPath(staged.getName())
                .build();
    }

    private File resolve(Uri uri) throws FileNotFoundException {
        Context context = getContext();
        String name = uri.getLastPathSegment();
        if (context == null || name == null || name.contains("/") || name.contains("..")) {
            throw new FileNotFoundException(String.valueOf(uri));
        }
        File file = new File(new File(context.getCacheDir(), SHARE_DIR), name);
        if (!file.isFile()) {
            throw new FileNotFoundException(String.valueOf(uri));
        }
        return file;
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!"r".equals(mode)) {
            throw new FileNotFoundException("read-only provider");
        }
        return ParcelFileDescriptor.open(resolve(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection,
            String[] selectionArgs, String sortOrder) {
        File file;
        try {
            file = resolve(uri);
        } catch (FileNotFoundException missing) {
            return null;
        }
        if (projection == null) {
            projection = new String[] {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
        }
        MatrixCursor cursor = new MatrixCursor(projection, 1);
        Object[] row = new Object[projection.length];
        for (int i = 0; i < projection.length; i++) {
            if (OpenableColumns.DISPLAY_NAME.equals(projection[i])) {
                row[i] = file.getName();
            } else if (OpenableColumns.SIZE.equals(projection[i])) {
                row[i] = file.length();
            }
        }
        cursor.addRow(row);
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        return "application/octet-stream";
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] selectionArgs) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] selectionArgs) {
        return 0;
    }
}
