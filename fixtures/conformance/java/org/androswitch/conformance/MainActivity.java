package org.androswitch.conformance;
import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.view.View;
import android.widget.*;
import java.io.*;
public final class MainActivity extends Activity implements View.OnClickListener {
  private Journal journal;
  private Handler handler;
  private TextView summary, last, detail;
  private boolean running, waiting, resumeQueue;
  private int group, renderer, pendingAsync = -1, resultCursor;
  private int order;
  private static final String[] GROUPS = {"All", "DEX", "Text", "Collections", "TimeUnit",
      "Exceptions", "Reflection", "Threads", "Math", "Numbers", "CPU", "Horizon",
      "Android identity", "Clock", "Display", "Package", "Storage", "Preferences", "Bundle", "UI",
      "Input", "SQLite", "Audio", "Lifecycle", "JNI", "Native", "OpenGL ES", "Vulkan"};
  @Override
  public void onCreate(Bundle b) {
    super.onCreate(b);
    RunBook.activity = this;
    handler = new Handler();
    try {
      journal = new Journal(getFilesDir());
      exclusions = Files.read(new File(getFilesDir(), "exclude.txt"));
    } catch (IOException e) {
      throw new IllegalStateException(e);
    }
    LinearLayout root = new LinearLayout(this);
    root.setOrientation(1);
    summary = line(root, "");
    last = line(root, "Ready; reports saved in application files/conformance");
    detail = line(root, "Runtime facts and guest Android identity are separate");
    LinearLayout row = new LinearLayout(this);
    row.setOrientation(0);
    root.addView(row);
    button(row, 1, "Run all / resume");
    button(row, 6, "Stop and save");
    row = new LinearLayout(this);
    row.setOrientation(0);
    root.addView(row);
    button(row, 2, "Previous category");
    button(row, 3, "Next category");
    row = new LinearLayout(this);
    row.setOrientation(0);
    root.addView(row);
    button(row, 4, "Run category");
    button(row, 5, "Renderer tests: GLES / Vulkan");
    row = new LinearLayout(this);
    row.setOrientation(0);
    root.addView(row);
    button(row, 7, "Reset results");
    button(row, 8, "Export report");
    row = new LinearLayout(this);
    row.setOrientation(0);
    root.addView(row);
    button(row, 9, "Previous result");
    button(row, 10, "Next result");
    setContentView(root);
    update();
    try {
      if (Files.clean(Files.read(new File(getFilesDir(), "autorun.txt"))).equals("1")) {
        filter = Files.clean(Files.read(new File(getFilesDir(), "filter.txt")));
        if (filter.length() == 0)
          filter = "All";
        journal.next = 0;
        running = true;
        handler.post(step);
      }
    } catch (IOException e) {
      last.setText(e.toString());
    }
  }
  private TextView line(LinearLayout root, String value) {
    TextView v = new TextView(this);
    v.setText(value);
    v.setTextSize(12);
    root.addView(v);
    return v;
  }
  private void button(LinearLayout root, int id, String text) {
    Button b = new Button(this);
    b.setId(id);
    b.setText(text);
    b.setTextSize(12);
    b.setOnClickListener(this);
    root.addView(b);
  }
  private void update() {
    int pass = 0, other = 0, done = 0;
    for (String r : journal.results)
      if (r != null) {
        done++;
        if (Files.resultStatus(r).equals("PASS"))
          pass++;
        else
          other++;
      }
    summary.setText("Conformance | " + GROUPS[group] + " | done " + done + "/" + Catalog.IDS.length
        + " pass " + pass + " other " + other);
  }
  private void start(boolean selected) throws IOException {
    if (waiting) {
      last.setText("Waiting for the current asynchronous case");
      return;
    }
    running = false;
    journal.next = 0;
    filter = selected ? GROUPS[group] : "All";
    if (selected)
      journal.clearGroup(filter);
    Files.write(new File(getFilesDir(), "filter.txt"), filter);
    Files.write(new File(getFilesDir(), "autorun.txt"), "1");
    running = true;
    handler.post(step);
  }
  private String filter = "All", exclusions = "";
  private final Runnable step = new Runnable() {
    public void run() {
      if (!running || waiting)
        return;
      try {
        int i = journal.next;
        while (i < Catalog.IDS.length) {
          if (journal.results[i] != null
              || (!filter.equals("All") && !filter.equals(Catalog.GROUPS[i]))) {
            i++;
            continue;
          }
          if (Files.hasLine(exclusions, Catalog.IDS[i])) {
            journal.record(i, "NOT_RUN", 0,
                "Explicit test-control exclusion (exclude.txt); API was not executed");
            i++;
            continue;
          }
          break;
        }
        if (i >= Catalog.IDS.length) {
          running = false;
          journal.next = Catalog.IDS.length;
          journal.save();
          Files.write(new File(getFilesDir(), "autorun.txt"), "0");
          last.setText("Selected run complete; other categories may be NOT_RUN");
          update();
          return;
        }
        journal.begin(i);
        last.setText(Catalog.IDS[i]);
        if (Catalog.IDS[i].equals("activity.roundtrip")
            || Catalog.IDS[i].equals("activity.handler-order")) {
          waiting = true;
          pendingAsync = i;
          beginAsync(i);
          return;
        }
        String result = RunBook.execute(i);
        int x = Files.find(result, '\t', 0), y = Files.find(result, '\t', x + 1);
        journal.record(i, Files.slice(result, 0, x),
            Files.positiveLong(Files.slice(result, x + 1, y)),
            Files.slice(result, y + 1, result.length()));
        journal.save();
        resultCursor = i;
        showResult();
        update();
        handler.postDelayed(this, 20);
      } catch (IOException error) {
        running = false;
        last.setText("Journal error: " + error);
      }
    }
  };
  @Override
  public void onClick(View v) {
    try {
      switch (v.getId()) {
        case 1:
          start(false);
          break;
        case 2:
          group = (group + GROUPS.length - 1) % GROUPS.length;
          update();
          break;
        case 3:
          group = (group + 1) % GROUPS.length;
          update();
          break;
        case 4:
          start(true);
          break;
        case 5:
          renderer = 1 - renderer;
          group = renderer == 0 ? GROUPS.length - 2 : GROUPS.length - 1;
          update();
          break;
        case 6:
          resumeQueue = false;
          running = false;
          Files.write(new File(getFilesDir(), "autorun.txt"), "0");
          journal.save();
          last.setText("Stopped; saved report.json");
          break;
        case 7:
          running = false;
          resumeQueue = false;
          waiting = false;
          pendingAsync = -1;
          Files.write(new File(getFilesDir(), "autorun.txt"), "0");
          journal.reset();
          update();
          break;
        case 8:
          journal.save();
          File d = new File("/sdcard/Download/AndroSwitch-Conformance");
          if (!d.isDirectory() && !d.mkdirs())
            throw new IOException("export directory unavailable");
          Files.write(new File(d, "report.json"), Files.read(new File(journal.dir, "report.json")));
          last.setText("Exported Download/AndroSwitch-Conformance/report.json");
          break;
        case 9:
          resultCursor = (resultCursor + Catalog.IDS.length - 1) % Catalog.IDS.length;
          showResult();
          break;
        case 10:
          resultCursor = (resultCursor + 1) % Catalog.IDS.length;
          showResult();
          break;
      }
    } catch (IOException e) {
      last.setText(e.toString());
    }
  }

  private void showResult() {
    String result = journal.results[resultCursor];
    last.setText(Catalog.IDS[resultCursor] + " | "
        + (result == null ? "NOT_RUN" : Files.resultStatus(result)));
    detail.setText(result == null ? "Not executed" : Files.field(result, "detail"));
  }
  public void beginAsync(final int index) {
    pendingAsync = index;
    waiting = true;
    resumeQueue = running;
    RunBook.asyncResult = "";
    if (Catalog.IDS[index].equals("activity.roundtrip")) {
      Recorder.events = "";
      startActivity(new android.content.Intent(this, ChildActivity.class));
    } else {
      order = 0;
      handler.post(new Runnable() {
        public void run() {
          order = order * 10 + 1;
        }
      });
      handler.post(new Runnable() {
        public void run() {
          order = order * 10 + 2;
        }
      });
      handler.postDelayed(new Runnable() {
        public void run() {
          finishAsync(index, order == 12 ? "PASS" : "FAIL", "Handler order=" + order);
        }
      }, 100);
    }
  }
  private void finishAsync(int index, String status, String detail) {
    RunBook.asyncResult = status + "\t0\t" + detail;
    if (waiting && pendingAsync == index) {
      waiting = false;
      pendingAsync = -1;
      try {
        journal.record(index, status, 0, detail);
        journal.save();
        resultCursor = index;
        showResult();
        update();
        running = resumeQueue;
        if (running)
          handler.post(step);
      } catch (IOException error) {
        running = false;
        last.setText(error.toString());
      }
    }
  }
  @Override
  public void onResume() {
    super.onResume();
    if (waiting && pendingAsync >= 0 && Catalog.IDS[pendingAsync].equals("activity.roundtrip")) {
      final int i = pendingAsync;
      handler.postDelayed(new Runnable() {
        public void run() {
          finishAsync(i, Recorder.events.equals("CSRPTD") ? "PASS" : "FAIL",
              "Actual child lifecycle=" + Recorder.events);
        }
      }, 100);
    }
  }
  @Override
  public void onPause() {
    running = false;
    try {
      if (journal != null)
        journal.save();
    } catch (IOException e) {
    }
    super.onPause();
  }
}
