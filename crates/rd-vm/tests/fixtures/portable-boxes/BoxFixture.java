package org.androswitch.vm;

/** Ordinary Java APIs; the same source is run by the JVM and converted by d8. */
public final class BoxFixture {
    public static long observe(int id) {
        switch (id) {
            case 0: return Boolean.TRUE == Boolean.valueOf(true) ? 1 : 0;
            case 1: return Boolean.FALSE == Boolean.valueOf(false) ? 1 : 0;
            case 2: return Boolean.valueOf("TrUe").booleanValue() ? 1 : 0;
            case 3: return new Boolean(" true ").booleanValue() ? 1 : 0;
            case 4: return new Boolean((String) null).booleanValue() ? 1 : 0;
            case 5: return Boolean.logicalAnd(true, false) ? 1 : 0;
            case 6: return Boolean.logicalOr(true, false) ? 1 : 0;
            case 7: return Boolean.logicalXor(true, true) ? 1 : 0;
            case 8: return Boolean.TRUE.hashCode();
            case 9: return Boolean.FALSE.compareTo(Boolean.TRUE);
            case 10: return new Byte((byte) -128).longValue();
            case 11: return new Byte("127").intValue();
            case 12: return Byte.valueOf((byte) -128) == Byte.valueOf((byte) -128) ? 1 : 0;
            case 13: return new Short("-32768").longValue();
            case 14: return Short.valueOf((short) 127) == Short.valueOf((short) 127) ? 1 : 0;
            case 15: return new Character('\uffff').charValue();
            case 16: return Character.valueOf('A') == Character.valueOf('A') ? 1 : 0;
            case 17: return Character.valueOf('A').compareTo(Character.valueOf('B'));
            case 18: return new Integer("-2147483648").longValue();
            case 19: return new Integer("+2147483647").intValue();
            case 20: return Integer.valueOf(-128) == Integer.valueOf(-128) ? 1 : 0;
            case 21: return Integer.valueOf(127) == Integer.valueOf(127) ? 1 : 0;
            case 22: return Integer.valueOf(19).equals(Long.valueOf(19L)) ? 1 : 0;
            case 23: return new Integer(20).equals(Integer.valueOf(20)) ? 1 : 0;
            case 24: return new Long("-9223372036854775808").longValue();
            case 25: return Long.valueOf(127) == Long.valueOf(127) ? 1 : 0;
            case 26: return new Long(0x1234567887654321L).intValue();
            case 27: return new Long(-1L).hashCode();
            case 28: return new Float(" 1.25f ").intValue();
            case 29: return Float.floatToRawIntBits(new Float(-0.0).floatValue());
            case 30: return Float.floatToIntBits(Float.intBitsToFloat(0x7fa12345));
            case 31: return Float.floatToRawIntBits(Float.intBitsToFloat(0x7fa12345));
            case 32: return Float.valueOf(Float.NaN).equals(Float.valueOf(Float.intBitsToFloat(0x7fa12345))) ? 1 : 0;
            case 33: return Float.valueOf(-0.0f).equals(Float.valueOf(0.0f)) ? 1 : 0;
            case 34: return Float.valueOf(-0.0f).compareTo(Float.valueOf(0.0f));
            case 35: return Float.valueOf(Float.POSITIVE_INFINITY).intValue();
            case 36: return Float.valueOf(Float.NEGATIVE_INFINITY).intValue();
            case 37: return Float.valueOf(Float.NaN).intValue();
            case 38: return Float.valueOf(Float.POSITIVE_INFINITY).byteValue();
            case 39: return Float.valueOf(Float.NEGATIVE_INFINITY).shortValue();
            case 40: return Double.valueOf(Double.POSITIVE_INFINITY).intValue();
            case 41: return Double.valueOf(Double.NEGATIVE_INFINITY).intValue();
            case 42: return Double.valueOf(Double.NaN).longValue();
            case 43: return Double.valueOf(2147483648.0).intValue();
            case 44: return Double.valueOf(2147483648.0).byteValue();
            case 45: return Double.valueOf(2147483648.0).shortValue();
            case 46: return Double.valueOf(Double.POSITIVE_INFINITY).longValue();
            case 47: return Double.valueOf(-0.0).compareTo(Double.valueOf(0.0));
            case 48: return Double.doubleToLongBits(Double.longBitsToDouble(0x7ff123456789abcdL));
            case 49: return Double.doubleToRawLongBits(Double.longBitsToDouble(0x7ff123456789abcdL));
            case 50: return Double.valueOf(Double.NaN).hashCode();
            case 51: return new Double(" -1.25e2D ").intValue();
            case 52: return Double.doubleToRawLongBits(new Double("-0.0").doubleValue());
            case 53: return Float.isFinite(2.0f) ? 1 : 0;
            case 54: return Double.isInfinite(Double.POSITIVE_INFINITY) ? 1 : 0;
            case 55: return Double.isNaN(Double.NaN) ? 1 : 0;
            case 56: return new Integer(128) == new Integer(128) ? 1 : 0;
            case 57: return ((Number) Integer.valueOf(1024)).byteValue();
            case 58: return new Short((short) -3).compareTo(Short.valueOf((short) 5));
            case 59: return Byte.compare((byte) -1, (byte) 1);
            case 74: return Float.floatToIntBits(new Float("1.000000059604644775390626").floatValue());
            case 75: return Float.floatToIntBits(new Float("-1.000000059604644775390626").floatValue());
            default: return exceptional(id);
        }
    }

    private static long exceptional(int id) {
        try {
            switch (id) {
                case 60: new Byte("128"); break;
                case 61: new Short("-32769"); break;
                case 62: new Integer("2147483648"); break;
                case 63: new Long("9223372036854775808"); break;
                case 64: new Integer(" 3 "); break;
                case 65: new Integer(""); break;
                case 66: new Integer((String) null); break;
                case 67: new Long((String) null); break;
                case 68: new Double((String) null); break;
                case 69: new Float((String) null); break;
                case 70: Integer.valueOf(1).compareTo(null); break;
                case 71: new Double("NaNF"); break;
                case 72: new Float("1ff"); break;
                case 73: new Byte((String) null); break;
                default: throw new AssertionError(id);
            }
            return 0;
        } catch (NumberFormatException expected) { return 1; }
          catch (NullPointerException expected) { return 2; }
    }

    public static String text(int id) {
        switch (id) {
            case 0: return Boolean.FALSE.toString();
            case 1: return new Byte((byte) -1).toString();
            case 2: return Short.valueOf((short) 32000).toString();
            case 3: return Character.valueOf('A').toString();
            case 4: return Integer.valueOf(-2147483648).toString();
            case 5: return Long.valueOf(-9223372036854775808L).toString();
            case 6: return Float.valueOf(-0.0f).toString();
            case 7: return Double.valueOf(1e7).toString();
            case 8: return Float.valueOf(Float.NaN).toString();
            case 9: return Double.valueOf(Double.NEGATIVE_INFINITY).toString();
            default: throw new AssertionError(id);
        }
    }

    public static void main(String[] args) {
        for (int id = 0; id < 76; id++) System.out.println("value\t" + id + "\t" + observe(id));
        for (int id = 0; id < 10; id++) System.out.println("text\t" + id + "\t" + text(id));
    }
}
