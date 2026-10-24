package org.androswitch.references;
/** Original Java reference contracts; the JNI transport proof is a separate APK. */
public final class ClassReferenceCases {
    public static Class<?> echo(Class<?> type) { System.gc(); return type; }
    public static Object echoObject(Object value) { System.gc(); return value; }
    public static Class<?> own() { return ClassReferenceCases.class; }
    public static Class<?> primitive() { return int.class; }
    public static Class<?> array() { return String[][].class; }
    public static void fail(Class<?> type) {
        if (type == ClassReferenceCases.class) throw new IllegalStateException("class callback failure");
        throw new AssertionError("wrong class argument");
    }
    public static void main(String[] args) {
        System.out.println("echo\t" + (echo(own()) == own() ? 1 : 0));
        System.out.println("object\t" + (echoObject(own()) == own() ? 1 : 0));
        System.out.println("own\t" + own().getName());
        System.out.println("class\t" + own().getClass().getName());
        System.out.println("primitive\t" + primitive().getName());
        System.out.println("array\t" + array().getName());
        System.out.println("distinct\t" + (own() != Object.class ? 1 : 0));
        try { fail(own()); throw new AssertionError("missing exception"); }
        catch (IllegalStateException expected) { System.out.println("exception\t" + expected.getMessage()); }
    }
}
