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

import org.eclipse.collections.api.factory.Lists;
import org.eclipse.collections.api.list.MutableList;
import org.finos.legend.engine.test.emit.EMITPhase;
import org.finos.legend.engine.test.emit.EMITResult;
import org.finos.legend.engine.test.emit.rust.EMITRustPhase;
import org.finos.legend.engine.test.emit.rust.EMITRustResult;

import java.util.Collections;
import java.util.List;

/**
 * Per-model diff between Java and Rust pipelines. Owns both raw
 * sides and the merged {@link EMITDiffPhaseResult} list, one per
 * conceptual phase.
 */
public final class EMITDiffResult
{
    private final String label;
    private final EMITResult javaResult;
    private final EMITRustResult rustResult;
    private final List<EMITDiffPhaseResult> diffs;

    EMITDiffResult(String label, EMITResult javaResult, EMITRustResult rustResult, List<EMITDiffPhaseResult> diffs)
    {
        this.label = label;
        this.javaResult = javaResult;
        this.rustResult = rustResult;
        this.diffs = Collections.unmodifiableList(diffs);
    }

    public String getLabel()
    {
        return this.label;
    }

    public EMITResult getJavaResult()
    {
        return this.javaResult;
    }

    public EMITRustResult getRustResult()
    {
        return this.rustResult;
    }

    public List<EMITDiffPhaseResult> getDiffs()
    {
        return this.diffs;
    }

    /**
     * Build the diff matrix. Today: one {@code PARSE_AND_COMPILE}
     * conceptual phase pairing Java's (PARSE + COMPILE) with the Rust
     * merged cell, plus four UNSUPPORTED rows for the downstream
     * phases.
     */
    public static EMITDiffResult build(String label, EMITResult javaResult, EMITRustResult rustResult)
    {
        MutableList<EMITDiffPhaseResult> diffs = Lists.mutable.empty();

        diffs.add(EMITDiffPhaseResult.forParseAndCompile(
                label,
                javaResult.getPhase(EMITPhase.PARSE),
                javaResult.getPhase(EMITPhase.COMPILE),
                rustResult.getPhase(EMITRustPhase.PARSE_AND_COMPILE)));

        diffs.add(EMITDiffPhaseResult.unsupported(label, EMITPhase.MODEL_GENERATION,
                javaResult.getPhase(EMITPhase.MODEL_GENERATION),
                EMITRustPhase.MODEL_GENERATION, rustResult.getPhase(EMITRustPhase.MODEL_GENERATION)));
        diffs.add(EMITDiffPhaseResult.unsupported(label, EMITPhase.FILE_GENERATION,
                javaResult.getPhase(EMITPhase.FILE_GENERATION),
                EMITRustPhase.FILE_GENERATION, rustResult.getPhase(EMITRustPhase.FILE_GENERATION)));
        diffs.add(EMITDiffPhaseResult.unsupported(label, EMITPhase.TEST_EXECUTION,
                javaResult.getPhase(EMITPhase.TEST_EXECUTION),
                EMITRustPhase.TEST_EXECUTION, rustResult.getPhase(EMITRustPhase.TEST_EXECUTION)));
        diffs.add(EMITDiffPhaseResult.unsupported(label, EMITPhase.PLAN_GENERATION,
                javaResult.getPhase(EMITPhase.PLAN_GENERATION),
                EMITRustPhase.PLAN_GENERATION, rustResult.getPhase(EMITRustPhase.PLAN_GENERATION)));

        return new EMITDiffResult(label, javaResult, rustResult, diffs);
    }
}
