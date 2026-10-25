package org.androswitch.conformance;
import java.io.*;
final class Journal {
  final File dir, parent;
  final String[] results = new String[Catalog.IDS.length];
  int next;
  Journal(File root) throws IOException {
    parent = new File(root, "conformance");
    dir = new File(parent, Files.slice(Catalog.DIGEST, 0, 12));
    if (!dir.isDirectory() && !dir.mkdirs())
      throw new IOException("cannot create suite journal");
    for (int i = 0; i < results.length; i++) {
      File f = new File(dir, "case-" + i + ".json");
      if (f.exists())
        results[i] = Files.read(f);
    }
    next = Files.number(new File(dir, "next.txt"), 0);
    int pending = Files.number(new File(dir, "pending.txt"), -1);
    if (pending >= 0 && pending < results.length) {
      if (results[pending] == null)
        record(pending, "INTERRUPTED", 0,
            "Previous execution ended before a result. See runtime session report for actual "
                + "cause.");
      next = next > pending + 1 ? next : pending + 1;
      new File(dir, "pending.txt").delete();
    }
  }
  void begin(int index) throws IOException {
    Files.write(new File(dir, "pending.txt"), Integer.toString(index));
    Files.write(new File(dir, "next.txt"), Integer.toString(index + 1));
    next = index + 1;
  }
  void record(int i, String status, long nanos, String value) throws IOException {
    results[i] = "{\"id\":" + Files.quote(Catalog.IDS[i])
        + ",\"group\":" + Files.quote(Catalog.GROUPS[i])
        + ",\"kind\":" + Files.quote(Catalog.KINDS[i]) + ",\"status\":" + Files.quote(status)
        + ",\"elapsed_ns\":" + nanos + ",\"detail\":" + Files.quote(value) + "}";
    Files.write(new File(dir, "case-" + i + ".json"), results[i]);
    new File(dir, "pending.txt").delete();
  }
  void save() throws IOException {
    StringBuilder b = new StringBuilder("{\"schema_version\":1,\"suite\":\"AndroSwitch "
        + "Conformance\",\"suite_revision\":1,\"coverage_scope\":\"Declared catalog only; not all "
        + "Android or a CTS certification\",\"reference_api\":26,\"sensor_tests\":false,\"total\":")
                          .append(results.length)
                          .append(",\"catalog_sha256\":")
                          .append(Files.quote(Catalog.DIGEST))
                          .append(",\"next\":")
                          .append(next)
                          .append(",\"cases\":[");
    for (int i = 0; i < results.length; i++) {
      if (i > 0)
        b.append(',');
      if (results[i] != null)
        b.append(results[i]);
      else
        b.append("{\"id\":").append(Files.quote(Catalog.IDS[i])).append(",\"status\":\"NOT_RUN\"}");
    }
    String report = b.append("]}\n").toString();
    Files.write(new File(dir, "report.json"), report);
    Files.write(new File(parent, "report.json"), report);
  }
  private void archive() throws IOException {
    File old = new File(dir, "report.json");
    if (!old.exists())
      return;
    String stem = "history-" + System.currentTimeMillis();
    File target = new File(dir, stem + ".json");
    int suffix = 0;
    while (target.exists()) {
      if (++suffix > 10000)
        throw new IOException("history name limit");
      target = new File(dir, stem + "-" + suffix + ".json");
    }
    Files.write(target, Files.read(old));
  }
  void clearGroup(String group) throws IOException {
    archive();
    for (int i = 0; i < results.length; i++) {
      if (!group.equals("All") && !Catalog.GROUPS[i].equals(group))
        continue;
      File file = new File(dir, "case-" + i + ".json");
      if (file.exists() && !file.delete())
        throw new IOException("case result deletion failed");
      results[i] = null;
    }
    File pending = new File(dir, "pending.txt");
    if (pending.exists() && !pending.delete())
      throw new IOException("pending reset failed");
    next = 0;
    Files.write(new File(dir, "next.txt"), "0");
    save();
  }
  void reset() throws IOException {
    clearGroup("All");
  }
}
