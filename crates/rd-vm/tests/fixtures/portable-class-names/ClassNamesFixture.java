package org.androswitch.vm;

/** Unmodified source for JVM names and DEX annotation contracts. */
public final class ClassNamesFixture {
    public static class Member { public static class Deep {} }
    public class Inner {}
    public static class Dollar$Member {}
    public interface Face {}
    final Object initializerAnonymous = new Object() {};

    public static String simple(Class<?> type) { return type.getSimpleName(); }
    public static String literalName() { return Member.Deep.class.getSimpleName(); }
    static Class<?>[] cases() {
        class Local$Name {}
        Class<?> anonymous = new Runnable() { public void run() {} }.getClass();
        Class<?> initializer = new ClassNamesFixture().initializerAnonymous.getClass();
        return new Class<?>[] {
            ClassNamesFixture.class, Member.class, Member.Deep.class, Inner.class,
            Dollar$Member.class, Dollar$Name.class, Face.class, Local$Name.class,
            anonymous, initializer,
            java.lang.reflect.Array.newInstance(anonymous, 0).getClass(),
            String[][].class, Member[][].class, int[][].class,
            boolean.class, byte.class, char.class, short.class, int.class,
            long.class, float.class, double.class, void.class
        };
    }
    static String descriptor(Class<?> type) {
        if (type.isArray()) return type.getName().replace('.', '/');
        if (type == boolean.class) return "Z";
        if (type == byte.class) return "B";
        if (type == char.class) return "C";
        if (type == short.class) return "S";
        if (type == int.class) return "I";
        if (type == long.class) return "J";
        if (type == float.class) return "F";
        if (type == double.class) return "D";
        if (type == void.class) return "V";
        return "L" + type.getName().replace('.', '/') + ";";
    }
    public static void main(String[] args) {
        for (Class<?> type : cases())
            System.out.println(descriptor(type) + "\t" + simple(type));
    }
}
final class Dollar$Name {}
