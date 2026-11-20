package io.github.okebuddy.wfplus;

import org.libsdl.app.SDLActivity;

/** The game's activity. SDL3 is linked statically into libmain.so (the Rust
 *  crate wf built as a cdylib), so that is the only native library. */
public class WarningForeverActivity extends SDLActivity {
    @Override
    protected String[] getLibraries() {
        return new String[] { "main" };
    }
}
