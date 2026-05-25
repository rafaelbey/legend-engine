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
 * Default {@link RustPhaseSurface} implementation that maps
 * {@link EMITRustPhase#PARSE_AND_COMPILE} to a single
 * {@code nativeInitContextWithClasspath} call through
 * {@link EMITRustBridge}. Captures wall-clock duration; releases the
 * context handle eagerly (we don't yet drive any downstream Rust
 * phases that would consume it).
 */
public final class MergedClasspathInitSurface implements RustPhaseSurface
{
    @Override
    public EMITRustPhaseResult parseAndCompile(EMITSourceSet sourceSet)
    {
        long start = System.currentTimeMillis();
        EMITRustBridge.SynthesizedContext context = null;
        try
        {
            context = EMITRustBridge.openContext(sourceSet);
            long elapsed = System.currentTimeMillis() - start;
            return EMITRustPhaseResult.success(EMITRustPhase.PARSE_AND_COMPILE, elapsed,
                    "context " + Long.toHexString(context.getContextPointer()));
        }
        catch (Throwable t)
        {
            long elapsed = System.currentTimeMillis() - start;
            String message = (t.getMessage() == null) ? t.getClass().getSimpleName() : t.getMessage();
            return EMITRustPhaseResult.failure(EMITRustPhase.PARSE_AND_COMPILE, elapsed, message, t);
        }
        finally
        {
            EMITRustBridge.closeContext(context);
        }
    }
}
