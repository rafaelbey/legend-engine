// Copyright 2026 Goldman Sachs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

package org.finos.legend.pure.rust;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;

/**
 * Minimal JNI stub for the engine cdylib (`liblegend_engine_jni`). This
 * class deliberately lives at <code>org.finos.legend.pure.rust.PureRustEvaluator</code>
 * because the JNI symbol resolution baked into the cdylib uses that exact
 * fully-qualified name — the cdylib exports
 * <code>Java_org_finos_legend_pure_rust_PureRustEvaluator_native*</code>
 * symbols inherited from the upstream <code>legend-pure-parser-jni</code>
 * crate that <code>legend-engine-rust/crates/jni</code> wraps.
 *
 * <p><b>Must NOT coexist on the same classpath as
 * <code>legend-pure-runtime-rust-evaluator</code>'s much-larger
 * <code>PureRustEvaluator</code> class</b> — both define the same FQN and
 * the JVM would pick one nondeterministically. The Maven module
 * <code>legend-engine-emit-rust</code> does not depend on rust-evaluator,
 * and the engine pom does not pull it transitively, so this scaffold is
 * safe today. Follow-up: add a non-PureRustEvaluator JNI symbol alias
 * to <code>legend-engine-rust/crates/jni</code> and migrate this stub
 * to that alias to eliminate the FQN squat.
 *
 * <p>The static block uses {@link System#load(String)} (absolute path)
 * driven by the <code>legend.engine.cdylib.path</code> system property
 * set by the module's surefire config. If the property is unset, the
 * file is missing, or loading fails for any reason, the class is left
 * in an unloaded state — {@link #isAvailable()} returns false and
 * {@link EMITRustBridge} short-circuits via JUnit Assumptions so tests
 * Skip rather than Fail. Default <code>mvn install</code> stays green
 * even with no cargo on PATH.
 */
public final class PureRustEvaluator
{
    private static final String CDYLIB_PROPERTY = "legend.engine.cdylib.path";

    private static final Throwable LOAD_FAILURE;

    static
    {
        Throwable failure = null;
        String path = System.getProperty(CDYLIB_PROPERTY);
        if ((path == null) || path.isEmpty())
        {
            failure = new IllegalStateException(
                    "System property '" + CDYLIB_PROPERTY + "' is not set. "
                            + "Build the engine cdylib with `cargo build -p legend-engine-rust-jni` "
                            + "in legend-engine-rust/ and pass the path via surefire.");
        }
        else
        {
            try
            {
                Path resolved = Paths.get(path).toAbsolutePath().normalize();
                if (!Files.isRegularFile(resolved))
                {
                    failure = new IllegalStateException(
                            "Engine cdylib not found at '" + resolved + "'. "
                                    + "Build it with `cargo build -p legend-engine-rust-jni` "
                                    + "in legend-engine-rust/.");
                }
                else
                {
                    System.load(resolved.toString());
                }
            }
            catch (Throwable t)
            {
                // Broad catch is deliberate — this static block runs the
                // FIRST time anyone references PureRustEvaluator (including
                // isAvailable()). An uncaught Throwable here bakes the
                // class as unloadable (ExceptionInInitializerError) for
                // the rest of the JVM, so the skip-test path never gets
                // a chance to observe loadFailure(). Catch everything,
                // store, let isAvailable() report cleanly.
                failure = t;
            }
        }
        LOAD_FAILURE = failure;
    }

    private PureRustEvaluator()
    {
    }

    /**
     * Returns true if the cdylib loaded successfully in this JVM.
     */
    public static boolean isAvailable()
    {
        return LOAD_FAILURE == null;
    }

    /**
     * Returns the cdylib load failure (null if loading succeeded). Useful
     * for surfacing the cause to test-skip messages.
     */
    public static Throwable loadFailure()
    {
        return LOAD_FAILURE;
    }

    /**
     * Initialize a Pure evaluation context from a serialized classpath
     * (TOML bytes — see <code>legend-engine-rust/legend-pure-classpath.toml</code>
     * for the schema). Returns an opaque handle that must be released
     * via {@link #nativeFreeContext(long)}.
     *
     * <p>Throws (or returns 0?) on parse/compile errors — exact failure
     * semantics inherited from the upstream
     * <code>legend-pure-parser-jni</code> crate; <code>EMITRustBridge</code>
     * catches all throwables and surfaces them as a phase failure.
     */
    public static native long nativeInitContextWithClasspath(byte[] classpathBytes);

    /**
     * Release a context handle previously returned by
     * {@link #nativeInitContextWithClasspath(byte[])}. Safe to call with
     * a 0 handle (no-op upstream).
     */
    public static native void nativeFreeContext(long contextPtr);
}
