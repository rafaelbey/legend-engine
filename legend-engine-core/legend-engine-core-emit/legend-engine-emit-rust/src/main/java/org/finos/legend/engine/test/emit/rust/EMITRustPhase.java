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

/**
 * Rust-side phase taxonomy. Intentionally <b>does not</b> mirror the
 * Java {@link org.finos.legend.engine.test.emit.EMITPhase} 1:1 — the
 * Rust engine's current JNI surface
 * ({@code nativeInitContextWithClasspath}) performs parse and compile
 * atomically, so we report one merged {@link #PARSE_AND_COMPILE} cell.
 * The diff layer compares it against the conjunction of Java's
 * {@code PARSE} and {@code COMPILE} phases. Later phases are
 * scaffolded as {@link #MODEL_GENERATION} / {@link #FILE_GENERATION} /
 * {@link #TEST_EXECUTION} / {@link #PLAN_GENERATION} but are reported
 * as {@code SKIPPED (unsupported)} until corresponding Rust entry
 * points exist upstream in legend-pure-rust.
 */
public enum EMITRustPhase
{
    INITIALIZATION,
    PARSE_AND_COMPILE,
    MODEL_GENERATION,
    FILE_GENERATION,
    TEST_EXECUTION,
    PLAN_GENERATION
}
