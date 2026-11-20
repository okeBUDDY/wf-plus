package io.github.okebuddy.wfplus;

import android.content.res.Configuration;
import android.hardware.display.DisplayManager;
import android.os.Build;
import android.os.Bundle;
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
 *  Android keeps a game at 60 Hz on high-refresh panels. On create and on
 *  every display or configuration change it picks the display mode with
 *  the current resolution and the highest refresh rate
 *  (preferredDisplayModeId), tells the native side that rate (for the
 *  options' FPS caps and ANativeWindow_setFrameRate, which the native side
 *  calls whenever it makes a surface, also after a resume), and on API 30+
 *  sets it on SDL's surface. Presenting stays Fifo (VSync). */
public class WarningForeverActivity extends SDLActivity implements DisplayManager.DisplayListener {
    private static final String TAG = "wf";

    /** System.loadLibrary name: lib + NATIVE_LIB + .so in the APK. */
    static final String NATIVE_LIB = "wfplus";

    /** The panel's top refresh rate at the current resolution (Rust). */
    private static native void nativeSetMaxRefreshRate(float hz);

    private DisplayManager displayManager;

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
        applyHighestRefreshRate("create");
        displayManager = (DisplayManager) getSystemService(DISPLAY_SERVICE);
        if (displayManager != null) {
            displayManager.registerDisplayListener(this, null);
        }
    }

    @Override
    protected void onDestroy() {
        if (displayManager != null) {
            displayManager.unregisterDisplayListener(this);
            displayManager = null;
        }
        super.onDestroy();
    }

    @Override
    public void onConfigurationChanged(Configuration newConfig) {
        super.onConfigurationChanged(newConfig);
        applyHighestRefreshRate("configuration");
    }

    @Override
    public void onDisplayAdded(int displayId) {}

    @Override
    public void onDisplayRemoved(int displayId) {}

    @Override
    public void onDisplayChanged(int displayId) {
        Display d = currentDisplay();
        if (d != null && d.getDisplayId() == displayId) {
            applyHighestRefreshRate("display");
        }
    }

    private Display currentDisplay() {
        if (Build.VERSION.SDK_INT >= 30) {
            return getDisplay();
        }
        return getWindowManager().getDefaultDisplay();
    }

    /** Picks the mode with the current resolution and the highest refresh
     *  rate; a no-op when it is already the preferred one. */
    private void applyHighestRefreshRate(String why) {
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
            } catch (UnsatisfiedLinkError e) {
                Log.w(TAG, "refresh: native library not loaded: " + e);
            }
            if (Build.VERSION.SDK_INT >= 30 && mSurface != null) {
                Surface s = mSurface.getHolder().getSurface();
                if (s != null && s.isValid()) {
                    s.setFrameRate(hz, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE);
                }
            }
        } catch (RuntimeException e) {
            Log.w(TAG, "refresh (" + why + "): " + e);
        }
    }
}
