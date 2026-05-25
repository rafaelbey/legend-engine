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

import org.finos.legend.engine.test.emit.EMITPhase;
import org.finos.legend.engine.test.emit.EMITPhaseResult;
import org.finos.legend.engine.test.emit.rust.EMITRustPhase;
import org.finos.legend.engine.test.emit.rust.EMITRustPhaseResult;

import java.util.Collections;
import java.util.List;

/**
 * Per-phase diff between Java EMIT and Rust EMIT outcomes.
 *
 * <p>The "phase" identifier names the conceptual phase (e.g.
 * {@code PARSE_AND_COMPILE}). The Java side may have produced 1..N
 * underlying {@link EMITPhaseResult}s for the same conceptual phase
 * (PARSE and COMPILE are conjoined when the Rust side reports them
 * as one). The Rust side has exactly one
 * {@link EMITRustPhaseResult}.
 */
public final class EMITDiffPhaseResult
{
    public enum Match
    {
        BOTH_PASS,
        BOTH_FAIL,
        JAVA_PASS_RUST_FAIL,
        JAVA_FAIL_RUST_PASS,
        UNSUPPORTED,
        JNI_UNAVAILABLE,
        JNI_ERROR
    }

    private final String label;
    private final List<EMITPhaseResult> javaResults;
    private final EMITRustPhaseResult rustResult;
    private final Match match;
    private final long javaMillis;
    private final long rustMillis;
    private final String divergence;

    EMITDiffPhaseResult(String label, List<EMITPhaseResult> javaResults, EMITRustPhaseResult rustResult,
                        Match match, long javaMillis, long rustMillis, String divergence)
    {
        this.label = label;
        this.javaResults = (javaResults == null) ? Collections.emptyList() : Collections.unmodifiableList(javaResults);
        this.rustResult = rustResult;
        this.match = match;
        this.javaMillis = javaMillis;
        this.rustMillis = rustMillis;
        this.divergence = divergence;
    }

    public String getLabel()
    {
        return this.label;
    }

    public List<EMITPhaseResult> getJavaResults()
    {
        return this.javaResults;
    }

    public EMITRustPhaseResult getRustResult()
    {
        return this.rustResult;
    }

    public Match getMatch()
    {
        return this.match;
    }

    public long getJavaMillis()
    {
        return this.javaMillis;
    }

    public long getRustMillis()
    {
        return this.rustMillis;
    }

    public String getDivergence()
    {
        return this.divergence;
    }

    /**
     * True when the match is a "hard" failure (must fail JUnit). Today:
     * Rust succeeded but Java failed (regression of unexpected nature),
     * or the JNI itself crashed.
     */
    public boolean isHardFailure()
    {
        return this.match == Match.JAVA_FAIL_RUST_PASS || this.match == Match.JNI_ERROR;
    }

    public String javaPhaseLabel()
    {
        if (this.javaResults.isEmpty())
        {
            return "—";
        }
        if (this.javaResults.size() == 1)
        {
            return this.javaResults.get(0).getPhase().name() + ":" + this.javaResults.get(0).getStatus();
        }
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < this.javaResults.size(); i++)
        {
            if (i > 0)
            {
                out.append('+');
            }
            EMITPhaseResult r = this.javaResults.get(i);
            out.append(r.getPhase().name()).append(':').append(r.getStatus());
        }
        return out.toString();
    }

    public String rustPhaseLabel()
    {
        if (this.rustResult == null)
        {
            return "—";
        }
        return this.rustResult.getPhase().name() + ":" + this.rustResult.getStatus();
    }

    public EMITRustPhase getConceptualPhase()
    {
        return (this.rustResult == null) ? null : this.rustResult.getPhase();
    }

    /**
     * Build a diff for the {@code PARSE_AND_COMPILE} cell: conjoins
     * Java's PARSE + COMPILE phases and compares against the Rust
     * merged cell.
     */
    static EMITDiffPhaseResult forParseAndCompile(String label, EMITPhaseResult javaParse,
                                                  EMITPhaseResult javaCompile, EMITRustPhaseResult rust)
    {
        java.util.List<EMITPhaseResult> javaList = new java.util.ArrayList<>(2);
        if (javaParse != null)
        {
            javaList.add(javaParse);
        }
        if (javaCompile != null)
        {
            javaList.add(javaCompile);
        }
        long javaMs = (javaParse == null ? 0L : javaParse.getDurationMs())
                + (javaCompile == null ? 0L : javaCompile.getDurationMs());
        long rustMs = (rust == null) ? 0L : rust.getDurationMs();

        boolean javaPass = ((javaParse == null) || javaParse.isSuccess())
                && ((javaCompile == null) || javaCompile.isSuccess())
                && (javaParse != null || javaCompile != null); // at least one ran
        EMITRustPhaseResult.Status rustStatus = (rust == null) ? null : rust.getStatus();

        Match m;
        String divergence = null;
        if (rust == null)
        {
            m = Match.JNI_ERROR;
            divergence = "no Rust phase result emitted";
        }
        else if (rustStatus == EMITRustPhaseResult.Status.JNI_UNAVAILABLE)
        {
            m = Match.JNI_UNAVAILABLE;
            divergence = rust.getMessage();
        }
        else if (rustStatus == EMITRustPhaseResult.Status.UNSUPPORTED)
        {
            m = Match.UNSUPPORTED;
        }
        else if (rustStatus == EMITRustPhaseResult.Status.SUCCESS)
        {
            m = javaPass ? Match.BOTH_PASS : Match.JAVA_FAIL_RUST_PASS;
            if (!javaPass)
            {
                divergence = "Java failed but Rust passed; phases=" + javaPhaseStatusList(javaList);
            }
        }
        else // FAILURE or SKIPPED
        {
            m = javaPass ? Match.JAVA_PASS_RUST_FAIL : Match.BOTH_FAIL;
            divergence = (rust.getMessage() == null) ? "rust phase " + rustStatus : rust.getMessage();
        }

        return new EMITDiffPhaseResult(label, javaList, rust, m, javaMs, rustMs, divergence);
    }

    private static String javaPhaseStatusList(List<EMITPhaseResult> javaResults)
    {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < javaResults.size(); i++)
        {
            if (i > 0)
            {
                out.append(',');
            }
            EMITPhaseResult r = javaResults.get(i);
            out.append(r.getPhase().name()).append('=').append(r.getStatus());
        }
        return out.toString();
    }

    static EMITDiffPhaseResult unsupported(String label, EMITPhase javaPhase, EMITPhaseResult javaResult,
                                           EMITRustPhase rustPhase, EMITRustPhaseResult rustResult)
    {
        java.util.List<EMITPhaseResult> javaList = (javaResult == null) ? Collections.emptyList() : Collections.singletonList(javaResult);
        long javaMs = (javaResult == null) ? 0L : javaResult.getDurationMs();
        long rustMs = (rustResult == null) ? 0L : rustResult.getDurationMs();
        return new EMITDiffPhaseResult(label, javaList, rustResult, Match.UNSUPPORTED, javaMs, rustMs, null);
    }
}
