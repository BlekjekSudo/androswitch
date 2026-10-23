package org.androswitch.persistence;
import android.app.Activity;
import android.os.Bundle;
import android.content.Context;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import android.database.sqlite.SQLiteOpenHelper;
import android.widget.TextView;

public class DatabaseActivity extends Activity {
    private static final class Notes extends SQLiteOpenHelper {
        Notes(Context context) { super(context, "notes.db", null, 1); }
        public void onCreate(SQLiteDatabase db) {
            System.gc();
            db.execSQL("CREATE TABLE notes(id INTEGER PRIMARY KEY, text TEXT NOT NULL, wide INTEGER, data BLOB)");
        }
        public void onUpgrade(SQLiteDatabase db, int oldVersion, int newVersion) {
            throw new IllegalStateException("unexpected upgrade");
        }
    }
    private int permissions;
    public void onRequestPermissionsResult(int code, String[] names, int[] results) {
        if (code == 7 && results.length == 2 && results[0] == 0 && results[1] == -1) permissions = 1;
    }
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        Notes notes = new Notes(this);
        SQLiteDatabase db = notes.getWritableDatabase();
        if (db != notes.getReadableDatabase() || db.getVersion() != 1) throw new IllegalStateException("helper identity/version");
        db.beginTransaction();
        db.execSQL("INSERT INTO notes(text,wide,data) VALUES(?,?,?)", new Object[]{"saved 雪", Long.valueOf(5000000000L), new byte[]{0,-1,42}});
        db.setTransactionSuccessful();
        db.endTransaction();
        db.beginTransaction();
        db.execSQL("DELETE FROM notes");
        db.endTransaction(); // rollback
        Cursor cursor = db.rawQuery("SELECT text,wide,data FROM notes WHERE text=?", new String[]{"saved 雪"});
        System.gc();
        int count = cursor.getCount();
        if (!cursor.moveToFirst() || cursor.getColumnIndex("wide") != 1 || cursor.getLong(1) != 5000000000L || cursor.getBlob(2)[1] != -1 || !"saved 雪".equals(cursor.getString(0))) throw new IllegalStateException("cursor values");
        cursor.close(); notes.close();
        boolean closed = false;
        try { db.execSQL("DELETE FROM notes"); } catch (IllegalStateException expected) { closed = true; }
        if (!closed) throw new IllegalStateException("closed database accepted writes");
        requestPermissions(new String[]{"android.permission.READ_EXTERNAL_STORAGE", "android.permission.CAMERA"}, 7);
        if (permissions != 1 || checkSelfPermission("android.permission.INTERNET") != -1) throw new IllegalStateException("capability permission policy");
        try {
            TextView label = new TextView(this);
            label.setText("Notes " + count + " / " + Contracts.reflection());
            setContentView(label);
        } catch (Exception failure) { throw new IllegalStateException(failure); }
    }
}
