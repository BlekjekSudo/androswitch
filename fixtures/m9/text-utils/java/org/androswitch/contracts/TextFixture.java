package org.androswitch.contracts;
import android.text.TextUtils;
public class TextFixture {
    public static class NativeCollector implements TextCases.Collector {public native void collect();}
    public static int run(int kind){
        TextCases.reset();
        if(kind==15)TextCases.hook=new NativeCollector();
        return TextCases.encode(TextUtils.isEmpty(TextCases.make(kind)));
    }
    public static int length(int kind){return TextCases.make(kind).length();}
    public static String caught(){
        try{run(14);return "no exception";}
        catch(IllegalStateException failure){return failure.getMessage();}
    }
    public static boolean query(CharSequence sequence){return TextUtils.isEmpty(sequence);}
    public static Object wrong(){return new TextCases.NonSequence();}
}
