package org.androswitch.ui;
public final class ProofApplication extends android.app.Application {
    public int created;
    @Override public void onCreate() { super.onCreate(); created++; }
}
