package org.androswitch.fixture;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;

/** Ordinary java.io calls. No AndroSwitch API or platform-specific fallback. */
public final class StorageProbe {
    public static int store(int value) throws Exception {
        File file = new File(System.getProperty("user.dir"), "save.bin");
        FileOutputStream output = new FileOutputStream(file);
        output.write(new byte[] {(byte)value, (byte)(value + 1), (byte)(value + 2)});
        output.close();
        FileOutputStream append = new FileOutputStream(file, true);
        append.write(value + 3);
        append.close();
        return read();
    }
    public static int read() throws Exception {
        File file = new File(System.getProperty("user.dir"), "save.bin");
        FileInputStream input = new FileInputStream(file);
        byte[] data = new byte[8];
        int count = input.read(data, 2, 4);
        int total = 0;
        for (int i = 2; i < 2 + count; i++) total = total * 257 + (data[i] & 255);
        int eof = input.read();
        input.close();
        return (int)file.length() == 4 && count == 4 && eof == -1 ? total : -1;
    }
    public static int obb() throws Exception {
        FileInputStream input = new FileInputStream("/storage/emulated/0/Android/obb/org.fixture.alpha/main.obb");
        long skipped = input.skip(65530L);
        int count = input.available();
        byte[] bytes = new byte[8];
        int read = input.read(bytes);
        input.close();
        return skipped == 65530L && count == 14 && read == 8 ? (bytes[0] & 255) * 256 + (bytes[7] & 255) : -1;
    }
    public static int mkdir() throws Exception {
        File parent = new File(System.getProperty("user.dir"), "missing/child");
        boolean direct = parent.mkdir();
        boolean recursive = parent.mkdirs();
        return !direct && recursive && parent.isDirectory() ? 1 : 0;
    }
}
