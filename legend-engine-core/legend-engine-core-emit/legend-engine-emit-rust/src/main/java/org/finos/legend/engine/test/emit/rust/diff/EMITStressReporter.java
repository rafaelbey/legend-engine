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

package org.finos.legend.engine.test.emit.rust.diff;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardOpenOption;
import java.util.HashSet;
import java.util.Set;

/**
 * Appends per-(model, phase) rows to a TSV report. The report path
 * defaults to {@code target/emit-rust-diff.tsv} relative to the current
 * working directory (which surefire sets to the module's basedir).
 * Override via the {@code emit.rust.report} system property.
 *
 * <p>Writes the header on first append per JVM (tracked via a static
 * set, so multiple test classes within one fork share the same header
 * decision). Multiple forks each get their own header — acceptable
 * for a v1; downstream aggregation can dedupe by line.
 */
public final class EMITStressReporter
{
    private static final String SYSTEM_PROPERTY = "emit.rust.report";
    private static final String DEFAULT_RELATIVE = "target/emit-rust-diff.tsv";
    private static final String HEADER =
            "model\tphase\tjava_status\trust_status\tmatch\tjava_ms\trust_ms\tdivergence";

    private static final Object LOCK = new Object();
    private static final Set<Path> HEADER_WRITTEN = new HashSet<>();

    private EMITStressReporter()
    {
    }

    public static Path reportPath()
    {
        String configured = System.getProperty(SYSTEM_PROPERTY);
        Path path = (configured == null || configured.isEmpty())
                ? Paths.get(DEFAULT_RELATIVE)
                : Paths.get(configured);
        return path.toAbsolutePath().normalize();
    }

    /**
     * Append all phase rows from a diff result to the TSV report.
     */
    public static void append(EMITDiffResult diff)
    {
        if (diff == null || diff.getDiffs().isEmpty())
        {
            return;
        }
        Path path = reportPath();
        synchronized (LOCK)
        {
            try
            {
                if (path.getParent() != null)
                {
                    Files.createDirectories(path.getParent());
                }
                if (HEADER_WRITTEN.add(path) && !Files.exists(path))
                {
                    Files.write(path, (HEADER + "\n").getBytes(StandardCharsets.UTF_8),
                            StandardOpenOption.CREATE, StandardOpenOption.TRUNCATE_EXISTING);
                }
                StringBuilder rows = new StringBuilder(256 * diff.getDiffs().size());
                for (EMITDiffPhaseResult d : diff.getDiffs())
                {
                    rows.append(escape(diff.getLabel())).append('\t')
                            .append(d.getConceptualPhase() == null ? "" : d.getConceptualPhase().name()).append('\t')
                            .append(escape(d.javaPhaseLabel())).append('\t')
                            .append(escape(d.rustPhaseLabel())).append('\t')
                            .append(d.getMatch()).append('\t')
                            .append(d.getJavaMillis()).append('\t')
                            .append(d.getRustMillis()).append('\t')
                            .append(escape(d.getDivergence()))
                            .append('\n');
                }
                Files.write(path, rows.toString().getBytes(StandardCharsets.UTF_8),
                        StandardOpenOption.APPEND);
            }
            catch (IOException e)
            {
                throw new UncheckedIOException("Failed to append EMIT Rust diff report at " + path, e);
            }
        }
    }

    private static String escape(String s)
    {
        if (s == null)
        {
            return "";
        }
        // TSV: tab/newline must be escaped or replaced; keep the field on one line
        return s.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }
}
