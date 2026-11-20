package io.github.okebuddy.wfplus;

import android.content.res.Configuration;
import android.hardware.display.DisplayManager;
import android.os.Build;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.view.Display;
import android.view.Surface;
import android.view.WindowManager;

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
 *  change relayouts the window, which must not land on SDL's startup. */
public class WarningForeverActivity extends SDLActivity implements DisplayManager.DisplayListener {
    private static final String TAG = "wf";

    /** System.loadLibrary name: lib + NATIVE_LIB + .so in the APK. */
    static final String NATIVE_LIB = "wfplus";

    /** How long after focus or a change the rate is applied. */
    private static final long APPLY_DELAY_MS = 750;

    /** The panel's top refresh rate at the current resolution (Rust). */
    private static native void nativeSetMaxRefreshRate(float hz);

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
}
