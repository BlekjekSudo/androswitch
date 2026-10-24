package org.androswitch.contracts;

/** Original pure-Java cases shared by the SDK fixture and JVM oracle. */
public class TextCases {
    public static int calls;
    public static int toStrings;
    public static Collector hook;
    public interface Collector {void collect();}
    public static void reset() {calls=0;toStrings=0;hook=null;}
    public static class Sequence implements CharSequence {
        private final int count;
        public Sequence(int count) {this.count=count;}
        public int length() {calls++;return count;}
        public char charAt(int at) {throw new AssertionError();}
        public CharSequence subSequence(int a,int b) {throw new AssertionError();}
        @Override public String toString() {toStrings++;throw new AssertionError();}
    }
    public static class OverrideSequence extends Sequence {
        public OverrideSequence(){super(7);}
        @Override public int length(){calls+=10;return 0;}
    }
    public static class ChangingSequence extends Sequence {
        public ChangingSequence(){super(7);}
        @Override public int length(){calls++;return calls==1?0:7;}
    }
    public static class ThrowingSequence extends Sequence {
        public ThrowingSequence(){super(7);}
        @Override public int length(){calls++;throw new IllegalStateException("length failure");}
    }
    public static class Payload {public int count;Payload(int count){this.count=count;}}
    public static class CollectingSequence extends Sequence {
        private final Payload payload=new Payload(2);
        public CollectingSequence(){super(7);}
        @Override public int length(){calls++;if(hook!=null)hook.collect();return payload.count;}
    }
    public static class NonSequence {public int length(){calls++;return 0;}}
    public static CharSequence make(int kind){
        switch(kind){
            case 0:return null;
            case 1:return "";
            case 2:return "x";
            case 3:return "\ud83d\ude03";
            case 4:return "\u0000";
            case 5:return " ";
            case 6:return new StringBuilder();
            case 7:return new StringBuilder("x");
            case 8:return new StringBuilder("\ud83d\ude03");
            case 9:return new StringBuffer("");
            case 10:return new Sequence(0);
            case 11:return new Sequence(3);
            case 12:return new OverrideSequence();
            case 13:return new ChangingSequence();
            case 14:return new ThrowingSequence();
            case 15:return new CollectingSequence();
            case 16:return new Sequence(-1);
            case 17:return new StringBuffer("xy");
            case 18:return new StringBuilder().append("\ud83d\ude03");
            default:throw new IllegalArgumentException("case");
        }
    }
    public static int encode(boolean empty){return (empty?1:0)+calls*10+toStrings*1000;}
}
