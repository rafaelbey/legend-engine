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

import org.finos.legend.engine.test.emit.EMITModelLoader;
import org.finos.legend.engine.test.emit.EMITResult;
import org.finos.legend.engine.test.emit.EMITRunner;
import org.finos.legend.engine.test.emit.EMITSourceSet;
import org.finos.legend.engine.test.emit.rust.EMITRustPhaseRunner;
import org.finos.legend.engine.test.emit.rust.EMITRustResult;

import java.nio.file.Path;

/**
 * Runs both the Java pipeline (via {@link EMITRunner}) and the Rust
 * pipeline (via {@link EMITRustPhaseRunner}) against the same source
 * set, builds the per-phase diff matrix.
 */
public final class EMITDiffRunner
{
    private final EMITModelLoader loader;
    private final EMITRunner javaRunner;
    private final EMITRustPhaseRunner rustRunner;

    public EMITDiffRunner()
    {
        this(new EMITModelLoader(), new EMITRunner(), new EMITRustPhaseRunner());
    }

    public EMITDiffRunner(EMITModelLoader loader, EMITRunner javaRunner, EMITRustPhaseRunner rustRunner)
    {
        this.loader = loader;
        this.javaRunner = javaRunner;
        this.rustRunner = rustRunner;
    }

    public EMITDiffResult runFromYaml(Path emitYaml) throws Exception
    {
        EMITSourceSet sourceSet = this.loader.load(emitYaml);
        String label = (sourceSet.getDescriptor() != null && sourceSet.getDescriptor().getName() != null)
                ? sourceSet.getDescriptor().getName()
                : emitYaml.getFileName().toString();

        // Java side: reuse EMITRunner verbatim
        EMITResult javaResult = this.javaRunner.run(sourceSet.getDescriptor());

        // Rust side
        EMITRustResult rustResult = this.rustRunner.run(sourceSet);

        return EMITDiffResult.build(label, javaResult, rustResult);
    }
}
