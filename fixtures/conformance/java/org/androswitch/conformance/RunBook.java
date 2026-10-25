package org.androswitch.conformance;
import android.app.Activity;
public final class RunBook {
  public static Activity activity;
  public static String asyncResult = "";
  public static void beginAsync(int index) {
    asyncResult = "";
    ((MainActivity) activity).beginAsync(index);
  }
  public static String pollAsync() {
    return asyncResult;
  }
  public static String execute(int index) {
    long start = System.nanoTime();
    String status = "PASS", value;
    try {
      if (index < Catalog.PURE_COUNT) {
        value = PureCases.run(index);
        if (!Catalog.EXPECTED[index].equals(value))
          throw new IllegalStateException(
              "expected=" + Catalog.EXPECTED[index] + " observed=" + value);
      } else if (index < Catalog.PURE_COUNT + Catalog.ANDROID_COUNT)
        value = AndroidCases.run(index - Catalog.PURE_COUNT, activity);
      else
        value = NativeCases.run(index - Catalog.PURE_COUNT - Catalog.ANDROID_COUNT);
      if (Files.prefix(value, "FAIL:")) {
        status = "FAIL";
        value = Files.slice(value, 5, value.length());
      } else if (Files.prefix(value, "UNSUPPORTED:")) {
        status = "UNSUPPORTED";
        value = Files.slice(value, 12, value.length());
      } else if (Files.prefix(value, "NOT_RUN:")) {
        status = "NOT_RUN";
        value = Files.slice(value, 8, value.length());
      }
    } catch (UnsatisfiedLinkError e) {
      status = "UNSUPPORTED";
      value = e.toString();
    } catch (NoClassDefFoundError e) {
      status = "BLOCKED";
      value = e.toString();
    } catch (NoSuchMethodError e) {
      status = "UNSUPPORTED";
      value = e.toString();
    } catch (NoSuchFieldError e) {
      status = "UNSUPPORTED";
      value = e.toString();
    } catch (Throwable e) {
      status = "FAIL";
      value = e.toString();
    }
    return status + "\t" + (System.nanoTime() - start) + "\t" + value;
  }
}
