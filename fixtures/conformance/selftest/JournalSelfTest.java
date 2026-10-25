package org.androswitch.conformance;
import java.io.File;
public final class JournalSelfTest {
  public static void main(String[] args) throws Exception {
    File root = new File(args[0]);
    if (!root.mkdir())
      throw new IllegalStateException("fresh test root required");
    File oversized = new File(root, "oversized");
    java.io.FileOutputStream out = new java.io.FileOutputStream(oversized);
    out.write(new byte[262144]);
    out.close();
    boolean bounded = false;
    try {
      Files.read(oversized);
    } catch (java.io.IOException expected) {
      bounded = true;
    }
    if (!bounded)
      throw new AssertionError("bounded reads");
    if (!Files.hasLine("a\r\nb\n", "b") || Files.hasLine("abc\n", "a"))
      throw new AssertionError("exact exclusion lines");
    Journal j = new Journal(root);
    if (j.next != 0)
      throw new AssertionError();
    j.begin(3);
    Journal recovered = new Journal(root);
    if (recovered.next != 4 || recovered.results[3] == null)
      throw new AssertionError("interruption checkpoint");
    recovered.record(0, "PASS", 12, "quote\" slash\\ newline\n tab\t euro\u20ac emoji\uD83D\uDE00");
    recovered.record(1, "FAIL", 7, "literal PASS is not success");
    recovered.save();
    if (!Files.resultStatus(recovered.results[1]).equals("FAIL"))
      throw new AssertionError("detail PASS must not alter status");
    if (!Files.field(recovered.results[0], "detail").contains("\\u20ac"))
      throw new AssertionError("escaped detail");
    recovered.record(Catalog.IDS.length - 1, "PASS", 1, "retained group");
    recovered.save();
    recovered.clearGroup(Catalog.GROUPS[0]);
    if (recovered.results[0] != null || recovered.results[Catalog.IDS.length - 1] == null)
      throw new AssertionError("category isolation");
    recovered.reset();
    if (recovered.next != 0 || recovered.results[0] != null)
      throw new AssertionError("reset");
    File[] history = recovered.dir.listFiles();
    int count = 0;
    for (File file : history)
      if (file.getName().startsWith("history-"))
        count++;
    if (count != 2)
      throw new AssertionError("history");
    recovered.begin(3);
    recovered = new Journal(root);
    recovered.record(0, "PASS", 12, "quote\" slash\\ newline\n tab\t euro\u20ac emoji\uD83D\uDE00");
    recovered.record(1, "FAIL", 7, "literal PASS is not success");
    recovered.save();
    System.out.println(new File(recovered.dir, "report.json").getAbsolutePath());
    System.out.println(Files.clean(" \t17\n").equals("17") && Files.find("a\tb", '\t', 0) == 1
        && Files.prefix("FAIL:detail", "FAIL:"));
  }
}
