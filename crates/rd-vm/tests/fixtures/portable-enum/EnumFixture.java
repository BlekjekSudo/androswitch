package org.androswitch.vm;

/** Runs unchanged on a JVM and as DEX in the portable interpreter. */
public final class EnumFixture {
    enum Alpha {
        FIRST, SECOND { @Override public String toString() { return "overridden"; } }, THIRD;
        static Alpha ALIAS = SECOND;
        static Alpha AAAPOISON;
    }
    enum Renamed { FIRST, SECOND, THIRD }
    enum Customized { ONE; @Override public String toString() { return super.toString(); } }
    enum FieldShadow { MEMBER; public String name = "shadow"; public int ordinal = 91; }
    public static int ordinals() { return Alpha.THIRD.ordinal() * 100 + Renamed.SECOND.ordinal() * 10; }
    @SuppressWarnings({"rawtypes", "unchecked"})
    public static int comparisons() {
        return Alpha.FIRST.compareTo(Alpha.THIRD) == -2
            && Alpha.SECOND.compareTo(Alpha.FIRST) == 1
            && ((Comparable)Alpha.SECOND).compareTo(Alpha.THIRD) == -1 ? 1 : 0;
    }
    public static int identities() {
        return Alpha.SECOND.getDeclaringClass() == Alpha.class
            && Alpha.SECOND.equals(Alpha.SECOND) && !Alpha.SECOND.equals(Renamed.SECOND)
            && Alpha.SECOND.hashCode() == ((Enum<?>)Alpha.SECOND).hashCode()
            && Enum.valueOf(Alpha.class,"SECOND") == Alpha.SECOND ? 1 : 0;
    }
    public static int arrayCopies() {
        Alpha[] one = Alpha.values(); Alpha[] two = Alpha.values(); one[0] = null;
        return one != two && two[0] == Alpha.FIRST ? 1 : 0;
    }
    public static int fieldShadow() {
        return FieldShadow.MEMBER.name == "shadow" && FieldShadow.MEMBER.ordinal == 91
            && FieldShadow.MEMBER.name() == "MEMBER" && FieldShadow.MEMBER.ordinal() == 0 ? 1 : 0;
    }
    public static int missing() {
        try { Enum.valueOf(Alpha.class,"MISSING"); return 0; }
        catch (IllegalArgumentException expected) { return 1; }
    }
    public static int aliasIsNotAConstant() {
        try { Enum.valueOf(Alpha.class,"ALIAS"); return 0; }
        catch (IllegalArgumentException expected) { return 1; }
    }
    @SuppressWarnings({"rawtypes", "unchecked"})
    public static int differentTypes() {
        try { ((Comparable)Alpha.SECOND).compareTo(Renamed.SECOND); return 0; }
        catch (ClassCastException expected) { return 1; }
    }
    public static int nullArgs() {
        int count = 0;
        try { Alpha.SECOND.compareTo(null); } catch (NullPointerException expected) { count++; }
        try { Enum.valueOf(null,"FIRST"); } catch (NullPointerException expected) { count++; }
        try { Enum.valueOf(Alpha.class,null); } catch (NullPointerException expected) { count++; }
        return count;
    }
    public static String defaultName() { return Alpha.FIRST.toString(); }
    public static String overrideName() { return Alpha.SECOND.toString(); }
    public static String superName() { return Customized.ONE.toString(); }
    public static String renamedName() { return Enum.valueOf(Renamed.class,"SECOND").name(); }
    public static void main(String[] arguments) {
        System.out.println(ordinals()); System.out.println(comparisons());
        System.out.println(identities()); System.out.println(arrayCopies());
        System.out.println(fieldShadow()); System.out.println(missing());
        System.out.println(aliasIsNotAConstant());
        System.out.println(differentTypes()); System.out.println(nullArgs());
        System.out.println(defaultName()); System.out.println(overrideName());
        System.out.println(superName()); System.out.println(renamedName());
    }
}
