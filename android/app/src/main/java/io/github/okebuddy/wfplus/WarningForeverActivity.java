package io.github.okebuddy.wfplus;

import org.libsdl.app.SDLActivity;

/** The game's activity. SDL3 is linked statically into libwfplus.so (the
 *  Rust crate wf built as a cdylib), so that is the only native library,
 *  and SDL_main is looked up in it. */
public class WarningForeverActivity extends SDLActivity {
    /** System.loadLibrary name: lib + NATIVE_LIB + .so in the APK. */
    static final String NATIVE_LIB = "wfplus";

    @Override
    protected String[] getLibraries() {
        return new String[] { NATIVE_LIB };
    }

    @Override
    protected String getMainSharedObject() {
        return getContext().getApplicationInfo().nativeLibraryDir + "/lib" + NATIVE_LIB + ".so";
    }
}
