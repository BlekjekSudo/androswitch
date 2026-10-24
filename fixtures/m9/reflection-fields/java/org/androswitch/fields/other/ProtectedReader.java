package org.androswitch.fields.other;
import org.androswitch.fields.FieldCases.Base;
public class ProtectedReader extends Base {
  public static int read(Base value) throws Exception {
    return Base.class.getDeclaredField("protectedNumber").getInt(value);
  }
}
