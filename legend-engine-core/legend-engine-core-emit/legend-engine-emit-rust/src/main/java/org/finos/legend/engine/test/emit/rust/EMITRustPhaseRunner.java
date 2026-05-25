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

import org.finos.legend.engine.test.emit.EMITSourceSet;

/**
 * Drives the Rust side of an EMIT model: initialization validation,
 * a single merged {@link EMITRustPhase#PARSE_AND_COMPILE} pass, and
 * placeholder {@link EMITRustPhaseResult.Status#UNSUPPORTED} entries
 * for the four downstream phases that have no Rust counterpart yet.
 *
 * <p>The phase surface is pluggable via the constructor. Default is
 * {@link MergedClasspathInitSurface} which uses the existing
 * {@code nativeInitContextWithClasspath} JNI entry point.
 */
public final class EMITRustPhaseRunner
{
    private final RustPhaseSurface phaseSurface;

    public EMITRustPhaseRunner()
    {
        this(new MergedClasspathInitSurface());
    }

    public EMITRustPhaseRunner(RustPhaseSurface phaseSurface)
    {
        this.phaseSurface = phaseSurface;
    }

    public EMITRustResult run(EMITSourceSet sourceSet)
    {
        EMITRustResult result = new EMITRustResult();
        if (sourceSet == null)
        {
            result.add(EMITRustPhaseResult.failure(EMITRustPhase.INITIALIZATION, 0L, "sourceSet is null", null));
            for (EMITRustPhase phase : EMITRustPhase.values())
            {
                if (phase != EMITRustPhase.INITIALIZATION)
                {
                    result.add(EMITRustPhaseResult.skipped(phase, "skipped due to initialization failure"));
                }
            }
            return result;
        }
        if (!EMITRustBridge.isAvailable())
        {
            String reason = EMITRustBridge.unavailabilityReason();
            for (EMITRustPhase phase : EMITRustPhase.values())
            {
                result.add(EMITRustPhaseResult.jniUnavailable(phase, reason));
            }
            return result;
        }

        result.add(EMITRustPhaseResult.success(EMITRustPhase.INITIALIZATION, 0L,
                sourceSet.getModelFiles().size() + " model files, " + sourceSet.getDependencyFiles().size() + " dependency files"));

        result.add(this.phaseSurface.parseAndCompile(sourceSet));

        result.add(EMITRustPhaseResult.unsupported(EMITRustPhase.MODEL_GENERATION));
        result.add(EMITRustPhaseResult.unsupported(EMITRustPhase.FILE_GENERATION));
        result.add(EMITRustPhaseResult.unsupported(EMITRustPhase.TEST_EXECUTION));
        result.add(EMITRustPhaseResult.unsupported(EMITRustPhase.PLAN_GENERATION));

        return result;
    }
}
