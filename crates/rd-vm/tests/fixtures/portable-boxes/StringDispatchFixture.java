package org.androswitch.vm;
/** Same String methods reached through String, Object and CharSequence. */
public final class StringDispatchFixture {
    static final String[] INPUT={"","hello","Aa","BB","\u0000","\ud83d\ude00","\uffff","e\u0301","\u4e2d\u6587","a/b/longer/path"};
    public static long observe(int id) {
        String s=INPUT[id/8];Object object=s;CharSequence sequence=s;
        switch(id%8) {
            case 0:return s.hashCode();
            case 1:return object.hashCode();
            case 2:return sequence.length();
            case 3:return object.equals(new String(s))?1:0;
            case 4:return object.equals(Integer.valueOf(1))?1:0;
            case 5:return object.toString()==s?1:0;
            case 6:return sequence.toString()==s?1:0;
            case 7:return s.isEmpty()?1:0;
        }
        throw new IllegalArgumentException("String dispatch scenario");
    }
    public static void main(String[] args) {
        for(int id=0;id<80;id++) System.out.println(id+"\t"+observe(id));
    }
}
