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

package org.finos.legend.engine.test.emit.rust.junit;

import org.eclipse.collections.api.factory.Lists;
import org.eclipse.collections.api.list.MutableList;
import org.finos.legend.engine.test.emit.rust.EMITRustBridge;
import org.finos.legend.engine.test.emit.rust.diff.EMITDiffPhaseResult;
import org.finos.legend.engine.test.emit.rust.diff.EMITDiffResult;
import org.finos.legend.engine.test.emit.rust.diff.EMITDiffRunner;
import org.finos.legend.engine.test.emit.rust.diff.EMITStressReporter;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.DynamicTest;

import java.nio.file.Path;
import java.util.List;
import java.util.stream.Stream;

/**
 * JUnit 5 integration for the EMIT Rust parity harness. Discovers
 * {@code *.emit.yaml} files on the classpath under {@code classpathRoot},
 * runs both Java and Rust pipelines per model, and emits one
 * {@link DynamicTest} per phase diff plus a final {@code Report} task
 * that appends to the TSV.
 *
 * <p>If the Rust JNI bridge is unavailable (cdylib not built or
 * classpath TOML missing), every parity-test for every model
 * Assumption-skips via {@link Assumptions#assumeTrue(boolean, String)}
 * — visible in JUnit output as Skipped rather than Failed.
 *
 * <pre>{@code
 *   @TestFactory
 *   Stream<DynamicTest> emit()
 *   {
 *       return EMITRustTestSuiteBuilder.taskStream("emit-models/");
 *   }
 * }</pre>
 */
public final class EMITRustTestSuiteBuilder
{
    private EMITRustTestSuiteBuilder()
    {
    }

    public static Stream<DynamicTest> taskStream(String classpathRoot)
    {
        return taskList(classpathRoot).stream();
    }

    public static List<DynamicTest> taskList(String classpathRoot)
    {
        MutableList<Path> yamls = EMITRustModelDiscovery.findEmitYamls(classpathRoot);
        EMITDiffRunner runner = new EMITDiffRunner();
        MutableList<DynamicTest> tasks = Lists.mutable.empty();
        for (Path yaml : yamls)
        {
            tasks.addAll(tasksFor(yaml, runner));
        }
        return tasks;
    }

    private static List<DynamicTest> tasksFor(Path yaml, EMITDiffRunner runner)
    {
        String label = stripExtension(yaml.getFileName().toString());
        MutableList<DynamicTest> tasks = Lists.mutable.empty();

        // If JNI is unavailable, emit a single skipped Diff test per model
        // and nothing else — keeps the JUnit tree readable.
        if (!EMITRustBridge.isAvailable())
        {
            String reason = EMITRustBridge.unavailabilityReason();
            tasks.add(DynamicTest.dynamicTest("[" + label + "] Diff: PARSE_AND_COMPILE",
                    () -> Assumptions.assumeTrue(false, "Rust JNI unavailable: " + reason)));
            return tasks;
        }

        EMITDiffResult diff;
        try
        {
            diff = runner.runFromYaml(yaml);
        }
        catch (Throwable t)
        {
            // Diff machinery itself blew up — surface as a single failing
            // test so the user sees the cause.
            tasks.add(DynamicTest.dynamicTest("[" + label + "] DiffRunner",
                    () ->
                    {
                        throw t;
                    }));
            return tasks;
        }

        for (EMITDiffPhaseResult phaseDiff : diff.getDiffs())
        {
            String name = "[" + label + "] Diff: " + (phaseDiff.getConceptualPhase() == null ? "(unknown)" : phaseDiff.getConceptualPhase().name());
            tasks.add(DynamicTest.dynamicTest(name, () -> assertDiff(phaseDiff)));
        }

        tasks.add(DynamicTest.dynamicTest("[" + label + "] Report",
                () -> EMITStressReporter.append(diff)));

        return tasks;
    }

    private static void assertDiff(EMITDiffPhaseResult diff)
    {
        switch (diff.getMatch())
        {
            case BOTH_PASS:
            case BOTH_FAIL:
                // Acceptable — same answer both sides
                return;
            case UNSUPPORTED:
                Assumptions.assumeTrue(false, diff.rustPhaseLabel() + " (Rust phase not yet implemented)");
                return;
            case JNI_UNAVAILABLE:
                Assumptions.assumeTrue(false, "Rust JNI unavailable: " + diff.getDivergence());
                return;
            case JAVA_PASS_RUST_FAIL:
                // Known gap — record but do not fail JUnit. Print to stderr
                // so the divergence is visible in the test output even
                // before the TSV report is consulted.
                System.err.println("[emit-rust] KNOWN GAP " + diff.getLabel() + " " + diff.getConceptualPhase()
                        + ": java=" + diff.javaPhaseLabel() + " rust=" + diff.rustPhaseLabel()
                        + " divergence=" + diff.getDivergence());
                return;
            case JAVA_FAIL_RUST_PASS:
                throw new AssertionError("Rust passed but Java failed for " + diff.getLabel()
                        + " phase " + diff.getConceptualPhase()
                        + " (java=" + diff.javaPhaseLabel() + ", rust=" + diff.rustPhaseLabel()
                        + "). Divergence: " + diff.getDivergence());
            case JNI_ERROR:
            default:
                throw new AssertionError("JNI error for " + diff.getLabel()
                        + " phase " + diff.getConceptualPhase()
                        + ": " + diff.getDivergence());
        }
    }

    private static String stripExtension(String fileName)
    {
        if (fileName.endsWith(".emit.yaml"))
        {
            return fileName.substring(0, fileName.length() - ".emit.yaml".length());
        }
        int lastDot = fileName.lastIndexOf('.');
        return (lastDot < 0) ? fileName : fileName.substring(0, lastDot);
    }
}
