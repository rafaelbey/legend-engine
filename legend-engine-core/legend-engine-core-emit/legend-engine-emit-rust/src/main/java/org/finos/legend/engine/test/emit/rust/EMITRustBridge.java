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

package org.finos.legend.engine.test.emit.rust;

import org.finos.legend.engine.test.emit.EMITSourceFile;
import org.finos.legend.engine.test.emit.EMITSourceSet;
import org.finos.legend.pure.rust.PureRustEvaluator;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.util.List;

/**
 * Lifecycle owner for Rust evaluation contexts opened via
 * {@link PureRustEvaluator#nativeInitContextWithClasspath(byte[])},
 * plus the classpath/descriptor synthesis the bridge needs to feed a
 * single EMIT model into the existing engine-wide classpath shape.
 *
 * <p>Synthesis flow per model — mirrors the real engine descriptor
 * layout under {@code src/main/resources/}:
 * <ol>
 *   <li>Create a temp staging root {@code stageRoot}.</li>
 *   <li>Stage every EMIT source file at
 *       <code>stageRoot/&lt;modelName&gt;/&lt;virtualPath&gt;</code>
 *       (sibling subdir named after the repo; arbitrary subpaths
 *       within — Pure file discovery is package-based, not
 *       path-based).</li>
 *   <li>Write a minimal <code>stageRoot/&lt;modelName&gt;.definition.json</code>
 *       with <code>{name, pattern: ".*", dependencies: ["platform"]}</code>.
 *       The permissive <code>.*</code> pattern lets the model declare
 *       any package — more specific repos (platform_*, core_*) win
 *       by virtue of stricter patterns.</li>
 *   <li>Read the workspace baseline <code>legend-pure-classpath.toml</code>
 *       (path passed via the <code>legend.pure.classpath</code> system
 *       property; default points at <code>legend-engine-rust/legend-pure-classpath.toml</code>),
 *       resolve every <code>descriptor = "..."</code> entry to an absolute
 *       path (workspace TOML uses workspace-root-relative paths), and
 *       append a synthetic <code>[[repo]]</code> entry for the EMIT
 *       model.</li>
 *   <li>UTF-8 encode and hand to {@code nativeInitContextWithClasspath}.</li>
 * </ol>
 *
 * <p>All filesystem state is in {@code java.io.tmpdir} and cleaned up
 * by {@link #closeContext(SynthesizedContext)} on success or on JVM exit
 * via {@link Runtime#addShutdownHook(Thread)} as a backstop.
 */
public final class EMITRustBridge
{
    private static final String CLASSPATH_TOML_PROPERTY = "legend.pure.classpath";

    private EMITRustBridge()
    {
    }

    /**
     * Returns true when both the cdylib has loaded successfully and the
     * baseline classpath TOML is present on disk.
     */
    public static boolean isAvailable()
    {
        if (!PureRustEvaluator.isAvailable())
        {
            return false;
        }
        Path baseline = baselineClasspath();
        return (baseline != null) && Files.isRegularFile(baseline);
    }

    /**
     * Short, user-facing reason the bridge is unavailable. Returns null
     * when {@link #isAvailable()} would return true.
     */
    public static String unavailabilityReason()
    {
        if (!PureRustEvaluator.isAvailable())
        {
            Throwable cause = PureRustEvaluator.loadFailure();
            return "cdylib unavailable: " + ((cause == null) ? "unknown" : cause.getMessage());
        }
        Path baseline = baselineClasspath();
        if (baseline == null)
        {
            return "system property '" + CLASSPATH_TOML_PROPERTY + "' not set";
        }
        if (!Files.isRegularFile(baseline))
        {
            return "baseline classpath TOML not found at '" + baseline + "'; "
                    + "run `cargo gen-classpath` in legend-engine-rust/";
        }
        return null;
    }

    /**
     * Open a Rust context populated with the engine-wide baseline classpath
     * plus a synthetic repo holding the given EMIT model's source files.
     */
    public static SynthesizedContext openContext(EMITSourceSet sourceSet) throws IOException
    {
        if (!isAvailable())
        {
            throw new IllegalStateException("EMITRustBridge unavailable: " + unavailabilityReason());
        }
        String modelName = synthesizedRepoName(sourceSet);
        Path stageRoot = Files.createTempDirectory("emit-rust-" + modelName + "-");
        try
        {
            // Layout mirrors real engine descriptors under src/main/resources/:
            //   stageRoot/<modelName>.definition.json   ← descriptor
            //   stageRoot/<modelName>/<virtualPath...>  ← Pure sources
            Path repoDir = stageRoot.resolve(modelName);
            Files.createDirectories(repoDir);

            stageFiles(sourceSet.getModelFiles(), repoDir);
            stageFiles(sourceSet.getDependencyFiles(), repoDir);

            Path descriptor = writeDescriptor(stageRoot, modelName);
            byte[] classpathBytes = buildClasspathToml(modelName, descriptor);

            long ptr = PureRustEvaluator.nativeInitContextWithClasspath(classpathBytes);
            return new SynthesizedContext(ptr, stageRoot);
        }
        catch (Throwable t)
        {
            // Cleanup if init throws — context handle, if non-zero, is unrecoverable
            // since we never observed it. Stage dir is ours; remove it.
            deleteSilently(stageRoot);
            throw t;
        }
    }

    /**
     * Release the context handle and clean up staged source files.
     */
    public static void closeContext(SynthesizedContext context)
    {
        if (context == null)
        {
            return;
        }
        try
        {
            if (context.contextPointer != 0L)
            {
                PureRustEvaluator.nativeFreeContext(context.contextPointer);
            }
        }
        finally
        {
            deleteSilently(context.stageRoot);
        }
    }

    // ----- helpers -----

    private static Path baselineClasspath()
    {
        String prop = System.getProperty(CLASSPATH_TOML_PROPERTY);
        return ((prop == null) || prop.isEmpty()) ? null : Paths.get(prop).toAbsolutePath().normalize();
    }

    private static String synthesizedRepoName(EMITSourceSet sourceSet)
    {
        String name = (sourceSet.getDescriptor() != null) ? sourceSet.getDescriptor().getName() : null;
        if (name == null || name.isEmpty())
        {
            return "emit_model";
        }
        // Sanitize to a Pure-repo-name-friendly identifier
        return name.replaceAll("[^A-Za-z0-9_]", "_");
    }

    private static void stageFiles(List<EMITSourceFile> sources, Path repoDir) throws IOException
    {
        for (EMITSourceFile file : sources)
        {
            Path target = repoDir.resolve(file.getVirtualPath()).normalize();
            if (!target.startsWith(repoDir))
            {
                throw new IOException("EMIT source virtual path escapes repo root: " + file.getVirtualPath());
            }
            Files.createDirectories(target.getParent());
            Files.copy(file.getAbsolutePath(), target, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static Path writeDescriptor(Path stageRoot, String modelName) throws IOException
    {
        // Real engine descriptors carry {name, pattern, dependencies}.
        // The repo's Pure files are discovered by walking a sibling
        // subdir named after the repo — already created in openContext.
        // pattern ".*" is intentionally permissive; stricter repos
        // (platform_*, core_*) own their own namespaces by stricter
        // patterns and the resolver picks the most specific match.
        // dependencies ["platform"] mirrors the minimal shape used by
        // core_functions_unclassified and friends.
        StringBuilder json = new StringBuilder(160);
        json.append("{\n");
        json.append("  \"name\": \"").append(escapeJson(modelName)).append("\",\n");
        json.append("  \"pattern\": \".*\",\n");
        json.append("  \"dependencies\": [\n");
        json.append("    \"platform\"\n");
        json.append("  ]\n");
        json.append("}\n");

        Path descriptor = stageRoot.resolve(modelName + ".definition.json");
        Files.write(descriptor, json.toString().getBytes(StandardCharsets.UTF_8));
        return descriptor;
    }

    private static byte[] buildClasspathToml(String modelName, Path syntheticDescriptor) throws IOException
    {
        Path baseline = baselineClasspath();
        Path baselineDir = baseline.getParent();
        StringBuilder out = new StringBuilder(8192);
        out.append("# Synthesized by EMITRustBridge — do not hand-edit.\n");
        out.append("# Derived from: ").append(baseline).append('\n');
        out.append("# Synthetic model: ").append(modelName).append('\n');
        out.append('\n');

        // Read the baseline TOML and rewrite each `descriptor = "<rel>"`
        // line to an absolute path resolved against the baseline TOML's
        // directory (workspace root). Pass other lines through verbatim.
        // Hand-written TOML parsing is deliberate — we don't pull in
        // jackson-dataformat-toml just for this.
        for (String line : Files.readAllLines(baseline, StandardCharsets.UTF_8))
        {
            String trimmed = line.trim();
            if (trimmed.startsWith("descriptor =") || trimmed.startsWith("descriptor="))
            {
                int eq = line.indexOf('=');
                int q1 = line.indexOf('"', eq);
                int q2 = (q1 < 0) ? -1 : line.indexOf('"', q1 + 1);
                if ((q1 < 0) || (q2 < 0))
                {
                    out.append(line).append('\n');
                    continue;
                }
                String relPath = line.substring(q1 + 1, q2);
                Path absolute = baselineDir.resolve(relPath).normalize();
                out.append("descriptor = \"").append(absolute.toString().replace("\\", "\\\\")).append("\"\n");
            }
            else
            {
                out.append(line).append('\n');
            }
        }

        // Append the synthetic model entry as the last [[repo]].
        out.append('\n');
        out.append("[[repo]]\n");
        out.append("name = \"").append(modelName).append("\"\n");
        out.append("kind = \"filesystem\"\n");
        out.append("descriptor = \"").append(syntheticDescriptor.toString().replace("\\", "\\\\")).append("\"\n");

        return out.toString().getBytes(StandardCharsets.UTF_8);
    }

    private static String escapeJson(String s)
    {
        StringBuilder out = new StringBuilder(s.length() + 8);
        for (int i = 0; i < s.length(); i++)
        {
            char c = s.charAt(i);
            switch (c)
            {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\n':
                    out.append("\\n");
                    break;
                case '\r':
                    out.append("\\r");
                    break;
                case '\t':
                    out.append("\\t");
                    break;
                default:
                    out.append(c);
            }
        }
        return out.toString();
    }

    private static void deleteSilently(Path root)
    {
        if ((root == null) || !Files.exists(root))
        {
            return;
        }
        try
        {
            Files.walk(root)
                    .sorted((a, b) -> b.getNameCount() - a.getNameCount())
                    .forEach(p ->
                    {
                        try
                        {
                            Files.deleteIfExists(p);
                        }
                        catch (IOException ignore)
                        {
                            // best-effort
                        }
                    });
        }
        catch (IOException ignore)
        {
            // best-effort
        }
    }

    /**
     * Opaque handle returned by {@link #openContext(EMITSourceSet)} —
     * owns both the Rust-side context pointer and the temp directory
     * of staged source files. Call {@link #closeContext(SynthesizedContext)}
     * to release both.
     */
    public static final class SynthesizedContext
    {
        private final long contextPointer;
        private final Path stageRoot;

        SynthesizedContext(long contextPointer, Path stageRoot)
        {
            this.contextPointer = contextPointer;
            this.stageRoot = stageRoot;
        }

        public long getContextPointer()
        {
            return this.contextPointer;
        }

        public Path getStageRoot()
        {
            return this.stageRoot;
        }
    }

    static
    {
        // Best-effort cleanup of any straggler temp dirs at JVM exit
        // (the per-context closer handles the success/failure paths,
        // but a hard crash mid-run would leak otherwise).
        Runtime.getRuntime().addShutdownHook(new Thread(() ->
        {
            try
            {
                Path tmp = Paths.get(System.getProperty("java.io.tmpdir"));
                if (Files.isDirectory(tmp))
                {
                    Files.list(tmp)
                            .filter(p -> p.getFileName().toString().startsWith("emit-rust-"))
                            .forEach(EMITRustBridge::deleteSilently);
                }
            }
            catch (IOException ignore)
            {
                // best-effort
            }
        }, "emit-rust-tmp-cleanup"));
    }
}
