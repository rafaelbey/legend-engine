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
 * SPI for the Rust-side phase implementations. The default
 * {@link MergedClasspathInitSurface} performs PARSE_AND_COMPILE as a
 * single atomic call to {@code nativeInitContextWithClasspath}. As
 * finer-grained JNI entry points land upstream in legend-pure-rust
 * (separate parse / compile / etc.), additional implementations can
 * be swapped in via {@link EMITRustPhaseRunner#EMITRustPhaseRunner(RustPhaseSurface)}.
 */
public interface RustPhaseSurface
{
    /**
     * Run PARSE+COMPILE for the given source set. Implementations
     * must return a phase result (never throw) — failures are
     * captured into {@link EMITRustPhaseResult#failure(EMITRustPhase, long, String, Throwable)}.
     */
    EMITRustPhaseResult parseAndCompile(EMITSourceSet sourceSet);
}
