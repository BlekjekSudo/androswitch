package org.androswitch.persistence;

import android.app.Activity;
import android.content.SharedPreferences;
import android.os.Bundle;
import android.widget.TextView;
import java.util.HashSet;
import java.util.Map;

public class PreferencesActivity extends Activity implements SharedPreferences.OnSharedPreferenceChangeListener {
    private int calls;
    public void onSharedPreferenceChanged(SharedPreferences preferences, String key) {
        calls++;
        System.gc();
    }
    protected void onCreate(Bundle bundle) {
        super.onCreate(bundle);
        SharedPreferences clearing = getSharedPreferences("clear-order", MODE_PRIVATE);
        clearing.edit().clear().putInt("old", 1).commit();
        clearing.edit().putInt("new", 2).clear().commit();
        if (clearing.contains("old") || clearing.getInt("new", 0) != 2) throw new IllegalStateException("clear-before-modifications");
        SharedPreferences preferences = getSharedPreferences("settings", MODE_PRIVATE);
        if (preferences != getSharedPreferences("settings", MODE_PRIVATE)) throw new IllegalStateException("canonical preferences");
        HashSet<String> colors = new HashSet<String>();
        colors.add("red"); colors.add("雪"); colors.add("red");
        if (!preferences.edit().putLong("wide", 5000000000L).putFloat("fraction", 1.25f)
                .putBoolean("enabled", true).putStringSet("colors", colors).commit()) {
            throw new IllegalStateException("durable commit");
        }
        preferences.registerOnSharedPreferenceChangeListener(this);
        int count = preferences.getInt("launches", 0) + 1;
        preferences.edit().putInt("launches", count).commit();
        preferences.edit().putString("deferred", "ready 雪 & <>").apply();
        if (!"ready 雪 & <>".equals(preferences.getString("deferred", "missing"))) throw new IllegalStateException("apply visibility");
        if (preferences.getLong("wide", 0) != 5000000000L || preferences.getFloat("fraction", 0) != 1.25f
                || !preferences.getBoolean("enabled", false) || preferences.getStringSet("colors", null).size() != 2) {
            throw new IllegalStateException("typed preferences");
        }
        Map<String,?> all = preferences.getAll();
        if (((Integer)all.get("launches")).intValue() != count) throw new IllegalStateException("getAll boxing");
        System.gc();
        TextView label = new TextView(this);
        label.setText("Prefs " + count + " / listener " + calls + " / " + Contracts.collections());
        setContentView(label);
    }
}
