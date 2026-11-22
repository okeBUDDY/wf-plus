package io.github.okebuddy.wfplus;

import android.content.Intent;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.Typeface;
import android.hardware.display.DisplayManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.Display;
import android.view.Gravity;
import android.view.Surface;
import android.view.ViewGroup;
import android.view.WindowManager;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.File;
import java.io.FileInputStream;
import java.nio.charset.StandardCharsets;

import org.libsdl.app.SDLActivity;

/** The game's activity. SDL3 is linked statically into libwfplus.so (the
 *  Rust crate wf built as a cdylib), so that is the only native library,
 *  and SDL_main is looked up in it.
 *
 *  It also asks for the panel's highest refresh rate: without a request
 *  Android keeps a game at 60 Hz on high-refresh panels. It picks the
 *  display mode with the current resolution and the highest refresh rate
 *  (preferredDisplayModeId), tells the native side that rate (for the
 *  options' FPS caps) and, on API 30+, sets it on SDL's surface
 *  (Surface.setFrameRate). Presenting stays Fifo (VSync).
 *
 *  None of that runs in onCreate or while SDL starts: it is posted to the
 *  UI thread a moment after the window gains focus (the surface exists by
 *  then; again after every resume) and after display or configuration
 *  changes, debounced, and nothing it calls may throw out of it. A mode
 *  change relayouts the window, which must not land on SDL's startup.
 *  The native side decides whether to ask at all (nativeRefreshState: not
 *  in safe mode, nor with refresh = off in wf-plus.ini or the intent).
 *
 *  Diagnostics without adb: while the native side keeps wf-status.txt in
 *  the app's files dir (safe mode, or no frame for 15 s), its text is shown
 *  over the game, selectable, polled every half second. The intent extras
 *  "backend" (vulkan|gl) and "refresh" (on|off) go to the native side
 *  before SDL_main starts. */
public class WarningForeverActivity extends SDLActivity implements DisplayManager.DisplayListener {

    /**
     * Back as a key, from the system Back of every navigation mode. SDL
     * traps Back (SDL_ANDROID_TRAP_BACK_BUTTON) and only a key event reaches
     * the game as one; the back gesture and Android 13+'s back dispatch call
     * onBackPressed with no key event, which SDL's trap then swallowed.
     */
    @Override
    public void onBackPressed() {
        SDLActivity.onNativeKeyDown(android.view.KeyEvent.KEYCODE_BACK);
        SDLActivity.onNativeKeyUp(android.view.KeyEvent.KEYCODE_BACK);
    }

    private static final String TAG = "wf";

    /** System.loadLibrary name: lib + NATIVE_LIB + .so in the APK. */
    static final String NATIVE_LIB = "wfplus";

    /** How long after focus or a change the rate is applied. */
    private static final long APPLY_DELAY_MS = 750;

    /** The panel's top refresh rate at the current resolution (Rust). */
    private static native void nativeSetMaxRefreshRate(float hz);

    /** Intent overrides: backend 0 none, 1 vulkan, 2 gl; refresh 0 none,
     *  1 on, 2 off. */
    private static native void nativeSetOverrides(int backend, int refresh);

    /** 0 not decided yet, 1 ask for the top refresh rate, 2 do not. */
    private static native int nativeRefreshState();

    /** onPause (true) / onResume (false), for the native watchdog. */
    private static native void nativeSetPaused(boolean paused);

    /** The overlay's close bar was tapped. */
    private static native void nativeDismissStatus();

    /** When this activity was created: an older status file is stale. */
    private long createdAt;

    /** How often the status file is polled. */
    private static final long STATUS_POLL_MS = 500;

    private LinearLayout statusView;
    private TextView statusText;
    private String statusShown;
    private final Runnable pollStatus = new Runnable() {
        @Override
        public void run() {
            updateStatus();
            ui.postDelayed(this, STATUS_POLL_MS);
        }
    };

    private DisplayManager displayManager;
    private final Handler ui = new Handler(Looper.getMainLooper());
    private String pendingWhy;
    private final Runnable apply = new Runnable() {
        @Override
        public void run() {
            applyHighestRefreshRate(pendingWhy);
        }
    };
    /** The surface and rate last given to Surface.setFrameRate. */
    private Surface rateSurface;
    private float rateHz;

    @Override
    protected String[] getLibraries() {
        return new String[] { NATIVE_LIB };
    }

    @Override
    protected String getMainSharedObject() {
        return getContext().getApplicationInfo().nativeLibraryDir + "/lib" + NATIVE_LIB + ".so";
    }

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (isFinishing()) {
            return;
        }
        try {
            Intent intent = getIntent();
            String backend = intent != null ? intent.getStringExtra("backend") : null;
            String refresh = intent != null ? intent.getStringExtra("refresh") : null;
            int b = "vulkan".equalsIgnoreCase(backend) ? 1 : "gl".equalsIgnoreCase(backend) ? 2 : 0;
            int r = "on".equalsIgnoreCase(refresh) ? 1 : "off".equalsIgnoreCase(refresh) ? 2 : 0;
            nativeSetOverrides(b, r);
            if (b != 0 || r != 0) {
                Log.i(TAG, "intent overrides: backend " + backend + ", refresh " + refresh);
            }
        } catch (Throwable t) {
            Log.w(TAG, "intent overrides: " + t);
        }
        // A status file left by an earlier process is stale (the native
        // side writes a new one in safe mode, after this).
        createdAt = System.currentTimeMillis();
        try {
            statusFile().delete();
        } catch (Throwable t) {
            Log.w(TAG, "status: " + t);
        }
        ui.postDelayed(pollStatus, STATUS_POLL_MS);
        try {
            displayManager = (DisplayManager) getSystemService(DISPLAY_SERVICE);
            if (displayManager != null) {
                displayManager.registerDisplayListener(this, ui);
            }
        } catch (Throwable t) {
            Log.w(TAG, "refresh: no display listener: " + t);
            displayManager = null;
        }
    }

    @Override
    protected void onDestroy() {
        ui.removeCallbacks(apply);
        ui.removeCallbacks(pollStatus);
        if (displayManager != null) {
            try {
                displayManager.unregisterDisplayListener(this);
            } catch (Throwable t) {
                Log.w(TAG, "refresh: " + t);
            }
            displayManager = null;
        }
        super.onDestroy();
    }

    @Override
    protected void onPause() {
        try {
            nativeSetPaused(true);
        } catch (Throwable t) {
            Log.w(TAG, "pause: " + t);
        }
        super.onPause();
    }

    @Override
    protected void onResume() {
        super.onResume();
        try {
            nativeSetPaused(false);
        } catch (Throwable t) {
            Log.w(TAG, "resume: " + t);
        }
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            // After a resume the Surface object is the same but its layer
            // may be new: set the rate on it again.
            rateSurface = null;
            schedule("focus");
        }
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        schedule("configuration");
    }

    @Override
    public void onDisplayAdded(int displayId) {}

    @Override
    public void onDisplayRemoved(int displayId) {}

    @Override
    public void onDisplayChanged(int displayId) {
        try {
            Display d = currentDisplay();
            if (d != null && d.getDisplayId() == displayId) {
                schedule("display");
            }
        } catch (Throwable t) {
            Log.w(TAG, "refresh (display): " + t);
        }
    }

    /** Applies the rate on the UI thread after APPLY_DELAY_MS, once for a
     *  burst of requests. */
    private void schedule(String why) {
        pendingWhy = why;
        ui.removeCallbacks(apply);
        ui.postDelayed(apply, APPLY_DELAY_MS);
    }

    private Display currentDisplay() {
        if (Build.VERSION.SDK_INT >= 30) {
            return getDisplay();
        }
        return getWindowManager().getDefaultDisplay();
    }

    /** Picks the mode with the current resolution and the highest refresh
     *  rate; changes nothing that is already set. */
    private void applyHighestRefreshRate(String why) {
        if (isFinishing()) {
            return;
        }
        try {
            int state = nativeRefreshState();
            if (state == 0) {
                schedule(why);
                return;
            }
            if (state != 1) {
                Log.i(TAG, "refresh (" + why + "): not requested (safe mode or refresh = off)");
                return;
            }
        } catch (Throwable t) {
            Log.w(TAG, "refresh: native side not reachable: " + t);
            return;
        }
        try {
            Display d = currentDisplay();
            if (d == null) {
                return;
            }
            Display.Mode current = d.getMode();
            Display.Mode best = current;
            for (Display.Mode m : d.getSupportedModes()) {
                if (m.getPhysicalWidth() == current.getPhysicalWidth()
                        && m.getPhysicalHeight() == current.getPhysicalHeight()
                        && m.getRefreshRate() > best.getRefreshRate()) {
                    best = m;
                }
            }
            float hz = best.getRefreshRate();
            WindowManager.LayoutParams lp = getWindow().getAttributes();
            if (lp.preferredDisplayModeId != best.getModeId()) {
                lp.preferredDisplayModeId = best.getModeId();
                getWindow().setAttributes(lp);
                Log.i(TAG, "refresh (" + why + "): mode " + best.getModeId() + " " + best.getPhysicalWidth() + "x"
                        + best.getPhysicalHeight() + " @ " + hz + " Hz (was " + current.getRefreshRate() + " Hz)");
            }
            try {
                nativeSetMaxRefreshRate(hz);
            } catch (Throwable t) {
                Log.w(TAG, "refresh: native side not reachable: " + t);
            }
            if (Build.VERSION.SDK_INT >= 30 && mSurface != null) {
                Surface s = mSurface.getHolder().getSurface();
                if (s != null && s.isValid() && (s != rateSurface || hz != rateHz)) {
                    s.setFrameRate(hz, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE);
                    rateSurface = s;
                    rateHz = hz;
                    Log.i(TAG, "refresh (" + why + "): Surface.setFrameRate(" + hz + ")");
                }
            }
        } catch (Throwable t) {
            Log.w(TAG, "refresh (" + why + "): " + t);
        }
    }

    /** The status file the native side writes (its storage dir: the
     *  external files dir, else the internal one). */
    private File statusFile() {
        File dir = getExternalFilesDir(null);
        if (dir == null) {
            dir = getFilesDir();
        }
        return new File(dir, "wf-status.txt");
    }

    /** Shows the status file's text over the game while it exists. */
    private void updateStatus() {
        String text = null;
        try {
            File f = statusFile();
            // Older than this activity: left by an earlier process.
            if (f.isFile() && f.lastModified() >= createdAt - 2000) {
                byte[] bytes = new byte[(int) Math.min(f.length(), 64 * 1024)];
                try (FileInputStream in = new FileInputStream(f)) {
                    int n = 0;
                    while (n < bytes.length) {
                        int r = in.read(bytes, n, bytes.length - n);
                        if (r < 0) {
                            break;
                        }
                        n += r;
                    }
                    text = new String(bytes, 0, n, StandardCharsets.UTF_8);
                }
            }
        } catch (Throwable t) {
            text = null;
        }
        try {
            if (text == null) {
                if (statusView != null) {
                    ((ViewGroup) statusView.getParent()).removeView(statusView);
                    statusView = null;
                    statusText = null;
                    statusShown = null;
                }
                return;
            }
            if (statusView == null) {
                ViewGroup root = (ViewGroup) getWindow().getDecorView();
                // A close bar on top (its tap closes the overlay), then the
                // text, selectable. The overlay covers only its own height
                // at the top; touches anywhere else reach the game.
                TextView close = new TextView(this);
                close.setText("\u2715  Tap here to close (android.status_overlay = off in wf-plus.ini turns this off)");
                close.setTextSize(13);
                close.setTextColor(Color.WHITE);
                close.setBackgroundColor(Color.argb(230, 96, 0, 0));
                close.setPadding(24, 20, 24, 20);
                close.setOnClickListener(v -> dismissStatus());
                statusText = new TextView(this);
                statusText.setTextIsSelectable(true);
                statusText.setTypeface(Typeface.MONOSPACE);
                statusText.setTextSize(12);
                statusText.setTextColor(Color.rgb(255, 255, 160));
                statusText.setPadding(24, 16, 24, 16);
                ScrollView scroll = new ScrollView(this);
                scroll.addView(statusText);
                statusView = new LinearLayout(this);
                statusView.setOrientation(LinearLayout.VERTICAL);
                statusView.setBackgroundColor(Color.argb(200, 0, 0, 48));
                statusView.addView(close, new LinearLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
                statusView.addView(scroll, new LinearLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT));
                FrameLayout.LayoutParams lp = new FrameLayout.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.TOP);
                root.addView(statusView, lp);
            }
            if (!text.equals(statusShown)) {
                statusText.setText(text);
                statusShown = text;
            }
        } catch (Throwable t) {
            Log.w(TAG, "status: " + t);
        }
    }

    /** Closes the overlay: the native side stops keeping the file (it
     *  shows it again only for a new report); the file is removed here too
     *  in case the native side is not reachable. */
    private void dismissStatus() {
        try {
            nativeDismissStatus();
        } catch (Throwable t) {
            Log.w(TAG, "status: " + t);
        }
        try {
            statusFile().delete();
        } catch (Throwable t) {
            Log.w(TAG, "status: " + t);
        }
        updateStatus();
    }
}
