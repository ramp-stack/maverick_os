package me.ramp.maverick;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;
import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.nio.file.Files;
import java.util.UUID;

public final class SharedFileProvider extends ContentProvider {
    public static Uri prepare(Context context, String path) throws IOException {
        File directory = new File(context.getCacheDir(), "maverick-share");
        if (!directory.isDirectory() && !directory.mkdirs()) throw new IOException("Cannot create shared files");
        File source = new File(path);
        File target = new File(directory, UUID.randomUUID() + "-" + source.getName());
        Files.copy(source.toPath(), target.toPath());
        return new Uri.Builder().scheme("content").authority(context.getPackageName() + ".maverick.files")
            .appendPath(target.getName()).build();
    }

    private File file(Uri uri) throws FileNotFoundException {
        try {
            if (!"content".equals(uri.getScheme()) ||
                !(getContext().getPackageName() + ".maverick.files").equals(uri.getAuthority()) ||
                uri.getPathSegments().size() != 1) throw new IOException("Invalid shared URI");
            File root = new File(getContext().getCacheDir(), "maverick-share").getCanonicalFile();
            File file = new File(root, uri.getLastPathSegment()).getCanonicalFile();
            if (!root.equals(file.getParentFile()) || !file.isFile()) throw new IOException("Invalid shared file");
            return file;
        } catch (IOException error) {throw new FileNotFoundException(error.getMessage());}
    }

    @Override public boolean onCreate() {return true;}
    @Override public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!mode.equals("r")) throw new FileNotFoundException("Shared files are read-only");
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }
    @Override public String getType(Uri uri) {
        String name = uri.getLastPathSegment();
        String type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(
            name.substring(name.lastIndexOf('.') + 1).toLowerCase(java.util.Locale.ROOT));
        return type == null ? "application/octet-stream" : type;
    }
    @Override public Cursor query(Uri uri, String[] columns, String selection, String[] args, String order) {
        try {
            File file = file(uri);
            if (columns == null) columns = new String[] {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};
            MatrixCursor cursor = new MatrixCursor(columns, 1);
            Object[] values = new Object[columns.length];
            for (int i = 0; i < columns.length; i++) {
                if (columns[i].equals(OpenableColumns.DISPLAY_NAME)) values[i] = file.getName();
                if (columns[i].equals(OpenableColumns.SIZE)) values[i] = file.length();
            }
            cursor.addRow(values);
            return cursor;
        } catch (FileNotFoundException error) {return null;}
    }
    @Override public Uri insert(Uri uri, ContentValues values) {throw new UnsupportedOperationException();}
    @Override public int update(Uri uri, ContentValues values, String selection, String[] args) {throw new UnsupportedOperationException();}
    @Override public int delete(Uri uri, String selection, String[] args) {throw new UnsupportedOperationException();}
}
