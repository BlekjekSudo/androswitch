package org.androswitch.vm;
import java.util.Base64;
import java.util.Locale;
/** The JVM oracle and DEX share this unchanged formatter call site. */
public final class FormatFixture {
    public static String render(String pattern, Object[] values) {
        return String.format(pattern,values);
    }
    public static String unlocalized(String pattern,Object[] values) {
        return String.format((Locale)null,pattern,values);
    }
    public static final class Named {
        @Override public String toString() { System.gc(); return "real DEX callback"; }
    }
    public static final class Fancy implements java.util.Formattable {
        public void formatTo(java.util.Formatter formatter,int flags,int width,int precision) {
            throw new UnsupportedOperationException("fixture requires actual formatTo dispatch");
        }
    }
    public static String callback(Object value) { return String.format("<%s>",value); }
    public static String nullArray() { return String.format("%s:%d",(Object[])null); }
    public static String literalNull() { return String.format("%s/%d/%b",null,null,null); }
    public static void main(String[] args) {
        Locale.setDefault(Locale.ROOT);
        Object[][] cases={
            {"Values: %d,%d,%d,%d,%d,%d",new Object[]{8,8,8,0,24,0}},
            {"%+06d|%(7d|% d",new Object[]{42,-42,42}},
            {"%#08x|%#06o|%X",new Object[]{42,9,-1}},
            {"%x|%o|%x",new Object[]{(byte)-1,(short)-1,-1L}},
            {"%2$s:%s:%<s:%1$d",new Object[]{7,"item"}},
            {"[%-8.3s]|%S|%.0s",new Object[]{"abcdef","upper","gone"}},
            {"%b:%B:%b:%b",new Object[]{null,false,"object",0}},
            {"%c:%C:%c",new Object[]{'z','q',0x1f600}},
            {"%5%:%n%s",new Object[]{"line"}},
            {"%05d:%X:%C:%s",new Object[]{null,null,null,null}},
            {"%d",new Object[]{Long.MIN_VALUE}},
            {"%3$s:%s",null},
            {"%d",new Object[]{"wrong"}},
            {"%s:%s",new Object[]{"one"}},
            {"%--5d",new Object[]{1}},
            {"%-d",new Object[]{1}},
            {"%#d",new Object[]{1}},
            {"%.2d",new Object[]{1}},
            {"%q",new Object[]{1}},
            {"%<s",new Object[]{"one"}},
            {"%+ d",new Object[]{1}},
            {"%c",new Object[]{-1}},
            {"%5n",new Object[]{}},
            {"%+n",new Object[]{}},
        };
        for(Object[] row:cases) {
            String result;
            try { result="S"+render((String)row[0],(Object[])row[1]); }
            catch(RuntimeException e) { result="E"+e.getClass().getSimpleName(); }
            // Android's %n is LF; the reference JVM runs on Windows here.
            result=result.replace(System.lineSeparator(),"\n");
            System.out.println(Base64.getEncoder().encodeToString(result.getBytes(java.nio.charset.StandardCharsets.UTF_8)));
        }
    }
}
