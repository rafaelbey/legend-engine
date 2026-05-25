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

import org.finos.legend.engine.test.emit.rust.junit.EMITRustTestSuiteBuilder;
import org.junit.jupiter.api.DynamicTest;
import org.junit.jupiter.api.TestFactory;

import java.util.stream.Stream;

/**
 * Drives every EMIT model under the {@code emit-models/} classpath
 * root through both the Java pipeline and the Rust pipeline, and
 * emits one JUnit dynamic test per phase diff. Reports skipped (with
 * a clear reason) when the engine cdylib is not built — default
 * {@code mvn install} stays green.
 */
public class RustEMITTestSuite
{
    @TestFactory
    Stream<DynamicTest> emit()
    {
        return EMITRustTestSuiteBuilder.taskStream("emit-models/");
    }
}
