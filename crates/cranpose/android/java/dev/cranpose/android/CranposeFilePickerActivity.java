package dev.cranpose.android;

import android.app.NativeActivity;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;

import java.io.IOException;

/**
 * A {@link NativeActivity} that exposes the Storage Access Framework to
 * Cranpose's native file picker.
 *
 * <p>Apps that want {@code cranpose_services::file_picker} on Android declare
 * this class (or a subclass) as their launcher activity. The Rust backend calls
 * {@link #cranposePickFile(long)} / {@link #cranposePickFolder(long)} /
 * {@link #cranposePickFolderStreaming(long)} over JNI; the chosen
 * {@code content://} document URIs are reported back <em>without copying any
 * data</em>. A picked file is later opened on demand through
 * {@link #cranposeOpenUri(String)}, which hands the provider's descriptor to
 * Rust, so even a multi-gigabyte folder is selected instantly and each file is
 * streamed only when it is actually read.
 *
 * <p>The folder picker uses {@code ACTION_OPEN_DOCUMENT_TREE}, so the user can
 * choose a folder served by any document provider the device exposes — local
 * storage, cloud, or a mounted WebDAV share — rather than a private path.
 *
 * <p>{@link #cranposePickFolderStreaming(long)} resolves the selection at once
 * and then walks the tree on a worker thread, reporting files in batches as it
 * finds them ({@link #nativeOnFolderEntries}). A slow provider (a mounted WebDAV
 * share with thousands of files) therefore no longer freezes the app: the first
 * tracks appear and can be played while the rest keep streaming in.
 */
public class CranposeFilePickerActivity extends NativeActivity {
    private static final int REQUEST_BASE = 0x0C9A0000;
    private static final int FLAG_FOLDER = 1;
    private static final int FLAG_STREAMING = 2;

    private long pendingToken;

    /** Number of files to accumulate before flushing a streaming batch. */
    private static final int FOLDER_BATCH_SIZE = 32;

    /** How many times to try listing a folder before skipping it (slow cloud /
     * WebDAV shares fail transiently). */
    private static final int FOLDER_QUERY_ATTEMPTS = 4;

    /** Base backoff between folder-listing retries, in ms (grows per attempt). */
    private static final int FOLDER_QUERY_RETRY_MS = 250;

    /** Implemented in the Rust cdylib. {@code entries} is newline-separated
     * {@code uri\tname} rows (one for a file, every descendant for a folder). */
    private static native void nativeOnFilePicked(
            long token, boolean folder, String entries, boolean cancelled, String error);

    /** A folder was selected (or cancelled/failed); streaming may now begin. */
    private static native void nativeOnFolderPicked(long token, boolean cancelled, String error);

    /** A streaming batch of discovered files ({@code uri\tname} rows). Returns
     * {@code false} once the consumer has gone away, so enumeration can stop. */
    private static native boolean nativeOnFolderEntries(long token, String entries);

    /** Folder enumeration finished, with an optional error. */
    private static native void nativeOnFolderFinished(long token, String error);

    /** Opens the document picker for a single file. Called from Rust over JNI. */
    public void cranposePickFile(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType("*/*");
        launch(intent, token, 0);
    }

    /** Opens the document tree picker for a folder, delivered all at once.
     * Called from Rust over JNI. */
    public void cranposePickFolder(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        launch(intent, token, FLAG_FOLDER);
    }

    /** Opens the document tree picker for a folder whose files stream back as
     * they are discovered. Called from Rust over JNI. */
    public void cranposePickFolderStreaming(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        launch(intent, token, FLAG_FOLDER | FLAG_STREAMING);
    }

    /**
     * Opens a picked {@code content://} document for reading and returns a
     * detached file descriptor. The Rust caller owns and closes it. Called over
     * JNI when a track is played, so nothing is copied up front.
     */
    public int cranposeOpenUri(String uriString) throws IOException {
        ParcelFileDescriptor descriptor =
                getContentResolver().openFileDescriptor(Uri.parse(uriString), "r");
        if (descriptor == null) {
            throw new IOException("no descriptor for " + uriString);
        }
        return descriptor.detachFd();
    }

    private void launch(Intent intent, long token, int flags) {
        try {
            startActivityForResult(intent, REQUEST_BASE | flags);
        } catch (Exception error) {
            if ((flags & FLAG_STREAMING) != 0) {
                nativeOnFolderPicked(token, false, error.toString());
            } else {
                nativeOnFilePicked(token, (flags & FLAG_FOLDER) != 0, null, false, error.toString());
            }
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if ((requestCode & 0xFFFF0000) != REQUEST_BASE) {
            return;
        }
        final long token = pendingToken;
        final int flags = requestCode & 0x0000FFFF;
        final boolean folder = (flags & FLAG_FOLDER) != 0;
        final boolean streaming = (flags & FLAG_STREAMING) != 0;
        final boolean ok = resultCode == RESULT_OK && data != null && data.getData() != null;

        if (streaming) {
            if (!ok) {
                nativeOnFolderPicked(token, true, null);
                return;
            }
            final Uri uri = data.getData();
            // Resolve the selection immediately so the UI stays responsive, then
            // walk the (possibly slow) tree off the main thread, streaming files
            // back in batches. This is what keeps a huge WebDAV folder from
            // freezing the app — the first tracks play while the rest arrive.
            nativeOnFolderPicked(token, false, null);
            new Thread(() -> {
                String error = null;
                try {
                    streamTree(uri, token);
                } catch (Exception failure) {
                    error = failure.toString();
                }
                nativeOnFolderFinished(token, error);
            }, "cranpose-folder-stream").start();
            return;
        }

        if (!ok) {
            nativeOnFilePicked(token, folder, null, true, null);
            return;
        }
        final Uri uri = data.getData();
        // Enumerating a tree only reads metadata (no byte copy), but a deep tree
        // can still take a moment, so resolve it off the main thread and never
        // block the UI — that previously froze the app on large folders.
        new Thread(() -> {
            try {
                String entries = folder ? enumerateTree(uri) : describeFile(uri);
                nativeOnFilePicked(token, folder, entries, false, null);
            } catch (Exception error) {
                nativeOnFilePicked(token, folder, null, false, error.toString());
            }
        }, "cranpose-file-picker").start();
    }

    private String describeFile(Uri uri) {
        return uri.toString() + "\t" + sanitize(displayName(uri, "file"));
    }

    private String enumerateTree(Uri treeUri) {
        try {
            getContentResolver().takePersistableUriPermission(
                    treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
        } catch (SecurityException ignored) {
        }
        StringBuilder out = new StringBuilder();
        collectTree(treeUri, DocumentsContract.getTreeDocumentId(treeUri), out);
        return out.toString();
    }

    private void collectTree(Uri treeUri, String documentId, StringBuilder out) {
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId);
        // Same resilience as the streaming walk: retry a slow provider and skip a
        // folder that stays unreadable rather than aborting the whole enumeration.
        Cursor cursor = queryChildrenWithRetry(childrenUri);
        try {
            if (cursor == null) {
                return;
            }
            while (cursor.moveToNext()) {
                String childId = cursor.getString(0);
                String name = cursor.getString(1);
                String mime = cursor.getString(2);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                    collectTree(treeUri, childId, out);
                } else {
                    Uri childUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId);
                    if (out.length() > 0) {
                        out.append('\n');
                    }
                    out.append(childUri.toString()).append('\t').append(sanitize(name));
                }
            }
        } finally {
            if (cursor != null) {
                cursor.close();
            }
        }
    }

    /** Walks {@code treeUri} depth-first, flushing files to Rust in batches as
     * they are found. Stops early if the consumer drops the stream. */
    private void streamTree(Uri treeUri, long token) {
        try {
            getContentResolver().takePersistableUriPermission(
                    treeUri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
        } catch (SecurityException ignored) {
        }
        Batch batch = new Batch(token);
        collectTreeStreaming(treeUri, DocumentsContract.getTreeDocumentId(treeUri), batch);
        batch.flush();
    }

    /** Returns {@code false} once the consumer has gone away, so recursion can
     * unwind without finishing the walk. */
    private boolean collectTreeStreaming(Uri treeUri, String documentId, Batch batch) {
        if (batch.stopped()) {
            return false;
        }
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, documentId);
        // Query this folder with retries. A slow document provider (a mounted
        // WebDAV / cloud share) transiently fails requests; without this, one
        // failed query would throw and abort the ENTIRE walk, so a big library
        // over a flaky link could end up adding nothing. On persistent failure we
        // skip just this folder and let the rest of the tree keep streaming in.
        Cursor cursor = queryChildrenWithRetry(childrenUri);
        if (cursor == null) {
            return !batch.stopped();
        }
        try {
            while (cursor.moveToNext()) {
                if (batch.stopped()) {
                    return false;
                }
                String childId = cursor.getString(0);
                String name = cursor.getString(1);
                String mime = cursor.getString(2);
                if (DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                    if (!collectTreeStreaming(treeUri, childId, batch)) {
                        return false;
                    }
                } else {
                    Uri childUri = DocumentsContract.buildDocumentUriUsingTree(treeUri, childId);
                    batch.add(childUri, name);
                }
            }
        } finally {
            cursor.close();
        }
        return !batch.stopped();
    }

    /** Lists a folder's children, retrying transient provider failures (a slow
     * cloud/WebDAV share throws intermittently). Returns {@code null} if the
     * folder cannot be read after {@link #FOLDER_QUERY_ATTEMPTS} tries, so the
     * caller skips just that folder instead of aborting the whole walk. */
    private Cursor queryChildrenWithRetry(Uri childrenUri) {
        String[] columns = {
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
        };
        for (int attempt = 1; ; attempt++) {
            try {
                return getContentResolver().query(childrenUri, columns, null, null, null);
            } catch (Exception error) {
                if (attempt >= FOLDER_QUERY_ATTEMPTS) {
                    android.util.Log.w(
                            "Cranpose",
                            "skipping unreadable folder after " + attempt + " tries: "
                                    + childrenUri + " (" + error + ")");
                    return null;
                }
                try {
                    Thread.sleep((long) FOLDER_QUERY_RETRY_MS * attempt);
                } catch (InterruptedException interrupted) {
                    Thread.currentThread().interrupt();
                    return null;
                }
            }
        }
    }

    /** Accumulates {@code uri\tname} rows and flushes them to Rust every
     * {@link #FOLDER_BATCH_SIZE} files, recording when the consumer stops. */
    private final class Batch {
        private final long token;
        private final StringBuilder rows = new StringBuilder();
        private int count;
        private boolean stopped;

        Batch(long token) {
            this.token = token;
        }

        void add(Uri uri, String name) {
            if (stopped) {
                return;
            }
            if (rows.length() > 0) {
                rows.append('\n');
            }
            rows.append(uri.toString()).append('\t').append(sanitize(name));
            count++;
            if (count >= FOLDER_BATCH_SIZE) {
                flush();
            }
        }

        void flush() {
            if (stopped || rows.length() == 0) {
                return;
            }
            boolean keepGoing = nativeOnFolderEntries(token, rows.toString());
            rows.setLength(0);
            count = 0;
            if (!keepGoing) {
                stopped = true;
            }
        }

        boolean stopped() {
            return stopped;
        }
    }

    private String displayName(Uri uri, String fallback) {
        try (Cursor cursor = getContentResolver().query(uri, null, null, null, null)) {
            if (cursor != null && cursor.moveToFirst()) {
                int column = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME);
                if (column >= 0) {
                    String name = cursor.getString(column);
                    if (name != null && !name.isEmpty()) {
                        return name;
                    }
                }
            }
        } catch (Exception ignored) {
        }
        return fallback;
    }

    /** The transport joins rows with newlines and fields with tabs, so strip
     * those from display names. */
    private String sanitize(String name) {
        if (name == null) {
            return "";
        }
        return name.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }
}
