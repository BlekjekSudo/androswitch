package org.androswitch.vm;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.UnsupportedEncodingException;

/** Unchanged SDK call sites used both by the JVM oracle and the portable DEX. */
public final class EncodingFixture {
    public static byte[] encode(String text, String charset) throws UnsupportedEncodingException {
        return text.getBytes(charset);
    }
    public static byte[] encodeDefault(String text) { return text.getBytes(); }
    public static byte[] afterGc(String text, String charset) throws UnsupportedEncodingException {
        byte[] bytes = text.getBytes(charset);
        System.gc();
        return bytes;
    }
    public static int checksum(byte[] bytes) {
        int result = 17;
        for (byte value : bytes) result = result * 31 + (value & 255);
        return result;
    }
    public static int separateArrays(String text, String charset) throws UnsupportedEncodingException {
        byte[] first = text.getBytes(charset);
        byte[] second = text.getBytes(charset);
        first[0] = (byte)(first[0] ^ 127);
        System.gc();
        return first != second && first[0] != second[0] ? checksum(second) : -1;
    }
    public static int catchIOException(String text, String charset) {
        try { text.getBytes(charset); return 0; }
        catch (IOException expected) { return 1; }
    }
    public static int catchAllocation(String text, String charset) throws UnsupportedEncodingException {
        try { text.getBytes(charset); return 0; }
        catch (OutOfMemoryError expected) { return 1; }
    }
    public static int writePayload(String text, String charset, String path) throws IOException {
        byte[] bytes = text.getBytes(charset);
        FileOutputStream output = new FileOutputStream(path);
        System.gc();
        output.write(bytes);
        output.close();
        System.gc();
        return checksum(bytes);
    }
    private static String hex(byte[] bytes) {
        StringBuilder result = new StringBuilder();
        final String digits = "0123456789abcdef";
        for (byte value : bytes) {
            result.append(digits.charAt((value & 255) >>> 4));
            result.append(digits.charAt(value & 15));
        }
        return result.toString();
    }
    public static void main(String[] args) throws Exception {
        String mixed = "A\u00e9\u4e2d\ud83d\ude00\u0000Z";
        String[][] cases = {
            {"ASCII", "UTF-8"}, {mixed, "UTF-8"}, {mixed, "utf8"}, {mixed, "uTf-8"},
            {mixed, "unicode-1-1-utf-8"}, {mixed, "UTF-16"}, {mixed, "UTF-16BE"},
            {mixed, "UTF-16LE"}, {mixed, "UnicodeBigUnmarked"}, {mixed, "UnicodeLittleUnmarked"},
            {"A\u00e9\u00ff\u4e2d\u0000Z", "ISO-8859-1"}, {"A\u00e9\u4e2d", "US-ASCII"},
            {"\u00e9\u00ff", "latin1"}, {"\u00e9\u00ff", "8859_1"}, {"A\u007f\u0080Z", "ASCII"},
            {"", "UTF-8"}, {"", "UTF-16"}, {"", "UTF-16BE"}, {"", "UTF-16LE"},
            {"", "ISO-8859-1"}, {"", "US-ASCII"}, {mixed, "DEFAULT"}, {"", "DEFAULT"},
            {"payload", null}, {"payload", "AndroSwitch-Unknown-Charset"}, {"payload", ""},
            {"payload", ":invalid?"}, {"\u0000\u0001\u007f\u0080\u00ff", "ISO-8859-1"},
            {"payload", "UTF-16BE"}, {"payload", "UTF_16"}, {"payload", "ISO8859_1"},
            // OpenJDK replaces a surrogate pair once; API 26 ART does it per
            // UTF-16 unit. These two rows are explicitly NOT equality oracles.
            {"A\ud83d\ude00Z", "US-ASCII"}, {"A\ud83d\ude00Z", "ISO-8859-1"},
        };
        for (int index = 0; index < cases.length; index++) {
            String text = cases[index][0], charset = cases[index][1];
            String actual;
            try { actual = "B" + hex("DEFAULT".equals(charset) ? encodeDefault(text) : encode(text, charset)); }
            catch (Exception error) { actual = "E" + error.getClass().getName(); }
            System.out.println(index + "\t" + hex(text.getBytes("UTF-8")) + "\t" +
                (charset == null ? "null" : hex(charset.getBytes("UTF-8"))) + "\t" + actual);
        }
    }
}
