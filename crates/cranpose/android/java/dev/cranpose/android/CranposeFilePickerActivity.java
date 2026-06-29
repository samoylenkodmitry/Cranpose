package dev.cranpose.android;

import android.app.NativeActivity;
import android.content.Intent;
import android.content.UriPermission;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.ParcelFileDescriptor;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;

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
    private static final int FLAG_WRITABLE = 4;

    /** A fixed-name probe used by {@link #cranposeFolderWritable}; created and
     * deleted immediately, and ignored by listings. */
    private static final String WRITABLE_PROBE_NAME = ".cranpose-write-probe";

    private long pendingToken;

    /** Number of files to accumulate before flushing a streaming batch. */
    private static final int FOLDER_BATCH_SIZE = 32;

    /** How many times to try listing a folder before skipping it (slow cloud /
     * WebDAV shares fail transiently). */
    private static final int FOLDER_QUERY_ATTEMPTS = 4;

    /** Base backoff between folder-listing retries, in ms (grows per attempt). */
    private static final int FOLDER_QUERY_RETRY_MS = 250;

    /** How many times to re-query a folder whose cursor reports
     * {@link DocumentsContract#EXTRA_LOADING} before giving up and using
     * whatever it returns (a network provider keeps the listing "loading" while
     * it fetches over the wire). */
    private static final int FOLDER_LOADING_ATTEMPTS = 24;

    /** Delay between {@link DocumentsContract#EXTRA_LOADING} re-queries, in ms
     * ({@link #FOLDER_LOADING_ATTEMPTS} × this ≈ the time budget for one folder
     * to finish loading). */
    private static final int FOLDER_LOADING_POLL_MS = 250;

    /** Records a granted selection in the native, process-static "resume inbox"
     * so a pick whose result arrives after the requesting activity (and the
     * native app) was destroyed is not lost. Android destroys and recreates the
     * activity when the SAF picker covers it on some devices, tearing down the
     * composition that was awaiting the result; the app drains this inbox on its
     * next start to recover the selection. {@code flags} are the request flags
     * ({@link #FLAG_FOLDER}/{@link #FLAG_STREAMING}). Implemented in the cdylib. */
    private static native void nativeRecordResumablePick(int flags, String uri, String name);

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

    /** A writable folder was picked (or cancelled/failed). {@code uri} is the
     * persisted SAF tree URI on success. */
    private static native void nativeOnWritableFolderPicked(
            long token, String uri, boolean cancelled, String error);

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

    /** Streams files under an <em>already granted</em> tree URI, without showing
     * any picker. Used by the resume path: a folder picked in a prior activity
     * instance that was destroyed before its walk ran is re-walked here once the
     * app restarts and reclaims the grant. Called from Rust over JNI. */
    public void cranposeStreamGrantedFolder(long token, String uriString) {
        final Uri uri = Uri.parse(uriString);
        new Thread(() -> {
            String error = null;
            try {
                streamTree(uri, token);
            } catch (Exception failure) {
                error = failure.toString();
            }
            nativeOnFolderFinished(token, error);
        }, "cranpose-folder-stream-resume").start();
    }

    /** Opens the document tree picker for a <em>writable</em> folder, taking a
     * persistent read/write grant. The chosen tree URI is reported back through
     * {@link #nativeOnWritableFolderPicked}. Called from Rust over JNI. */
    public void cranposePickWritableFolder(long token) {
        pendingToken = token;
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION);
        try {
            startActivityForResult(intent, REQUEST_BASE | FLAG_WRITABLE);
        } catch (Exception error) {
            nativeOnWritableFolderPicked(token, null, false, error.toString());
        }
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

    /** Writes (overwriting) {@code contents} to a file named {@code name} in the
     * writable tree. Returns 0 on success, 1 on permission failure (read-only),
     * 2 on any other error. Called from the Rust sync worker thread over JNI. */
    public int cranposeFolderWrite(String treeUriString, String name, byte[] contents) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, treeDocId);
            String docId = findWritableChildId(tree, treeDocId, name);
            Uri docUri;
            if (docId != null) {
                docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            } else {
                docUri = DocumentsContract.createDocument(
                        getContentResolver(), parent, "application/octet-stream", name);
                if (docUri == null) {
                    return 2;
                }
            }
            try (OutputStream output = getContentResolver().openOutputStream(docUri, "wt")) {
                if (output == null) {
                    return 2;
                }
                output.write(contents);
            }
            return 0;
        } catch (SecurityException error) {
            return 1;
        } catch (Exception error) {
            return 2;
        }
    }

    /** Lists immediate child file names (directories excluded), newline-joined.
     * Returns {@code null} only on a hard read failure (an empty folder is ""). */
    public String cranposeFolderList(String treeUriString) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, treeDocId);
            Cursor cursor = queryChildrenWithRetry(childrenUri);
            if (cursor == null) {
                return null;
            }
            StringBuilder out = new StringBuilder();
            try {
                while (cursor.moveToNext()) {
                    String name = cursor.getString(1);
                    String mime = cursor.getString(2);
                    if (name == null || DocumentsContract.Document.MIME_TYPE_DIR.equals(mime)) {
                        continue;
                    }
                    if (out.length() > 0) {
                        out.append('\n');
                    }
                    out.append(sanitize(name));
                }
            } finally {
                cursor.close();
            }
            return out.toString();
        } catch (Exception error) {
            return null;
        }
    }

    /** Reads the file {@code name} as bytes, or {@code null} if absent/unreadable. */
    public byte[] cranposeFolderRead(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            String docId = findWritableChildId(tree, treeDocId, name);
            if (docId == null) {
                return null;
            }
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            try (InputStream input = getContentResolver().openInputStream(docUri)) {
                if (input == null) {
                    return null;
                }
                ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                byte[] chunk = new byte[8192];
                int read;
                while ((read = input.read(chunk)) >= 0) {
                    buffer.write(chunk, 0, read);
                }
                return buffer.toByteArray();
            }
        } catch (Exception error) {
            return null;
        }
    }

    /** Deletes the file {@code name}. Returns 0 on success (or already gone), 2 otherwise. */
    public int cranposeFolderRemove(String treeUriString, String name) {
        try {
            Uri tree = Uri.parse(treeUriString);
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            String docId = findWritableChildId(tree, treeDocId, name);
            if (docId == null) {
                return 0;
            }
            Uri docUri = DocumentsContract.buildDocumentUriUsingTree(tree, docId);
            return DocumentsContract.deleteDocument(getContentResolver(), docUri) ? 0 : 2;
        } catch (Exception error) {
            return 2;
        }
    }

    /** Whether the tree is writable now: a persisted write grant exists AND a
     * probe document can be created and deleted (catching a read-only backing
     * store such as a read-only WebDAV mount). */
    public boolean cranposeFolderWritable(String treeUriString) {
        try {
            Uri tree = Uri.parse(treeUriString);
            boolean granted = false;
            for (UriPermission permission : getContentResolver().getPersistedUriPermissions()) {
                if (permission.getUri().equals(tree) && permission.isWritePermission()) {
                    granted = true;
                    break;
                }
            }
            if (!granted) {
                return false;
            }
            String treeDocId = DocumentsContract.getTreeDocumentId(tree);
            Uri parent = DocumentsContract.buildDocumentUriUsingTree(tree, treeDocId);
            String existing = findWritableChildId(tree, treeDocId, WRITABLE_PROBE_NAME);
            if (existing != null) {
                DocumentsContract.deleteDocument(
                        getContentResolver(),
                        DocumentsContract.buildDocumentUriUsingTree(tree, existing));
            }
            Uri probe = DocumentsContract.createDocument(
                    getContentResolver(), parent, "application/octet-stream", WRITABLE_PROBE_NAME);
            if (probe == null) {
                return false;
            }
            DocumentsContract.deleteDocument(getContentResolver(), probe);
            return true;
        } catch (Exception error) {
            return false;
        }
    }

    /** Finds the document id of a direct child by display name, or {@code null}. */
    private String findWritableChildId(Uri treeUri, String treeDocId, String name) {
        Uri childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, treeDocId);
        Cursor cursor = queryChildrenWithRetry(childrenUri);
        if (cursor == null) {
            return null;
        }
        try {
            while (cursor.moveToNext()) {
                if (name.equals(cursor.getString(1))) {
                    return cursor.getString(0);
                }
            }
        } finally {
            cursor.close();
        }
        return null;
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

        if ((flags & FLAG_WRITABLE) != 0) {
            if (!ok) {
                nativeOnWritableFolderPicked(token, null, true, null);
                return;
            }
            final Uri tree = data.getData();
            try {
                getContentResolver().takePersistableUriPermission(tree,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION
                                | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
                // Record the grant in the resume inbox before delivering it, so a
                // pick whose activity was recreated mid-prompt (tearing down the
                // awaiting composition) can still be reclaimed on the next start.
                // Live delivery below clears the inbox on the happy path.
                nativeRecordResumablePick(flags, tree.toString(), sanitize(displayName(tree, "folder")));
                nativeOnWritableFolderPicked(token, tree.toString(), false, null);
            } catch (Exception error) {
                nativeOnWritableFolderPicked(token, null, false, error.toString());
            }
            return;
        }

        if (streaming) {
            if (!ok) {
                nativeOnFolderPicked(token, true, null);
                return;
            }
            final Uri uri = data.getData();
            // Take a persistable grant and record the selection in the resume
            // inbox *before* delivering it, so that if this activity was recreated
            // (the SAF picker covered and destroyed it, tearing down the awaiting
            // composition) the next app start can still reclaim it. The live
            // delivery below clears the inbox on the happy path.
            try {
                getContentResolver().takePersistableUriPermission(
                        uri, Intent.FLAG_GRANT_READ_URI_PERMISSION);
            } catch (SecurityException ignored) {
            }
            nativeRecordResumablePick(flags, uri.toString(), sanitize(displayName(uri, "folder")));
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
        if (!folder) {
            // A single-file pick can also be lost to an activity recreation, so
            // record it for resume. (A persistable grant needs the request intent
            // to carry FLAG_GRANT_PERSISTABLE_URI_PERMISSION; the in-process grant
            // is enough for the common case where the process outlives the pick.)
            nativeRecordResumablePick(flags, uri.toString(), sanitize(displayName(uri, "file")));
        }
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
        int errorAttempt = 0;
        int loadingAttempt = 0;
        while (true) {
            Cursor cursor;
            try {
                cursor = getContentResolver().query(childrenUri, columns, null, null, null);
            } catch (Exception error) {
                errorAttempt++;
                if (errorAttempt >= FOLDER_QUERY_ATTEMPTS) {
                    android.util.Log.w(
                            "Cranpose",
                            "skipping unreadable folder after " + errorAttempt + " tries: "
                                    + childrenUri + " (" + error + ")");
                    return null;
                }
                if (!sleepQuietly((long) FOLDER_QUERY_RETRY_MS * errorAttempt)) {
                    return null;
                }
                continue;
            }
            if (cursor == null) {
                return null;
            }
            // A slow network document provider (RoundSync/rclone, a mounted WebDAV
            // share) returns an EMPTY cursor immediately with EXTRA_LOADING=true
            // while it fetches the real listing over the wire, then notifies and
            // serves the cached result on the next query. Reading the cursor right
            // now yields zero children, so the folder — even the picked root — looks
            // empty ("adds nothing"). Re-query until it stops loading (or we run out
            // of patience) instead of trusting the placeholder.
            if (isLoading(cursor) && loadingAttempt < FOLDER_LOADING_ATTEMPTS) {
                loadingAttempt++;
                cursor.close();
                if (!sleepQuietly(FOLDER_LOADING_POLL_MS)) {
                    return null;
                }
                continue;
            }
            return cursor;
        }
    }

    /** True if the provider flagged this cursor as still fetching its results. */
    private static boolean isLoading(Cursor cursor) {
        Bundle extras = cursor.getExtras();
        return extras != null && extras.getBoolean(DocumentsContract.EXTRA_LOADING, false);
    }

    /** Sleeps, returning {@code false} (so callers can bail) if interrupted. */
    private static boolean sleepQuietly(long millis) {
        try {
            Thread.sleep(millis);
            return true;
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            return false;
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
