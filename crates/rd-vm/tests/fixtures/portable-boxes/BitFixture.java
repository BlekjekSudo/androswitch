package org.androswitch.vm;
/** Integer/Long utility observations from a real JVM, unchanged for DEX. */
public final class BitFixture {
    static final int[] INTS={0,1,2,3,Integer.MIN_VALUE,Integer.MAX_VALUE,-1,-2,0x80000100,0x55555555,0xaaaaaaaa,0x80000001,0x7f000001,0x01020304};
    static final long[] LONGS={0,1,2,3,Long.MIN_VALUE,Long.MAX_VALUE,-1,-2,0x8000000000000100L,0x5555555555555555L,0xaaaaaaaaaaaaaaaaL,0x8000000000000001L,0x7f00000000000001L,0x0102030405060708L};
    static final int[] SHIFTS={Integer.MIN_VALUE,-65,-64,-33,-32,-1,0,1,31,32,33,63,64,65};
    public static long observe(int id) {
        if(id<266) {
            int n=id/19,op=id%19,a=INTS[n],b=INTS[(n+5)%14],d=SHIFTS[n];
            switch(op) {
                case 0:return Integer.bitCount(a);
                case 1:return Integer.numberOfLeadingZeros(a);
                case 2:return Integer.numberOfTrailingZeros(a);
                case 3:return Integer.highestOneBit(a);
                case 4:return Integer.lowestOneBit(a);
                case 5:return Integer.rotateLeft(a,d);
                case 6:return Integer.rotateRight(a,d);
                case 7:return Integer.reverse(a);
                case 8:return Integer.reverseBytes(a);
                case 9:return Integer.signum(a);
                case 10:return Integer.compare(a,b);
                case 11:return Integer.compareUnsigned(a,b);
                case 12:return Integer.divideUnsigned(a,b==0?1:b);
                case 13:return Integer.remainderUnsigned(a,b==0?1:b);
                case 14:return Integer.toUnsignedLong(a);
                case 15:return Integer.max(a,b);
                case 16:return Integer.min(a,b);
                case 17:return Integer.sum(a,b);
                case 18:return Integer.hashCode(a);
            }
        } else {
            int n=(id-266)/18,op=(id-266)%18,d=SHIFTS[n];long a=LONGS[n],b=LONGS[(n+5)%14];
            switch(op) {
                case 0:return Long.bitCount(a);
                case 1:return Long.numberOfLeadingZeros(a);
                case 2:return Long.numberOfTrailingZeros(a);
                case 3:return Long.highestOneBit(a);
                case 4:return Long.lowestOneBit(a);
                case 5:return Long.rotateLeft(a,d);
                case 6:return Long.rotateRight(a,d);
                case 7:return Long.reverse(a);
                case 8:return Long.reverseBytes(a);
                case 9:return Long.signum(a);
                case 10:return Long.compare(a,b);
                case 11:return Long.compareUnsigned(a,b);
                case 12:return Long.divideUnsigned(a,b==0?1:b);
                case 13:return Long.remainderUnsigned(a,b==0?1:b);
                case 14:return Long.max(a,b);
                case 15:return Long.min(a,b);
                case 16:return Long.sum(a,b);
                case 17:return Long.hashCode(a);
            }
        }
        throw new IllegalArgumentException("bit utility scenario");
    }
    public static int zero() {
        int caught=0;
        try{Integer.divideUnsigned(1,0);}catch(ArithmeticException e){caught++;}
        try{Integer.remainderUnsigned(1,0);}catch(ArithmeticException e){caught++;}
        try{Long.divideUnsigned(1,0);}catch(ArithmeticException e){caught++;}
        try{Long.remainderUnsigned(1,0);}catch(ArithmeticException e){caught++;}
        return caught;
    }
    public static void main(String[] args) {
        for(int id=0;id<518;id++) System.out.println(id+"\t"+observe(id));
        System.out.println("zero\t"+zero());
    }
}
