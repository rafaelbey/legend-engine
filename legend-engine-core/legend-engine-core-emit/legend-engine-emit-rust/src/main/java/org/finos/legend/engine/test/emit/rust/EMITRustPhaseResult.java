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
 * Rust-side per-phase outcome. Parallels
 * {@link org.finos.legend.engine.test.emit.EMITPhaseResult} but with a
 * simpler structure — no opaque {@code outputs} list since the Rust
 * side currently produces just a context handle.
 *
 * <p>The {@link Status#UNSUPPORTED} status is unique to the Rust side
 * and represents phases that exist in the Java pipeline but have no
 * Rust counterpart yet. It is treated as Skipped by JUnit and produces
 * a {@code UNSUPPORTED} row in the diff matrix.
 */
public final class EMITRustPhaseResult
{
    public enum Status
    {
        SUCCESS,
        FAILURE,
        SKIPPED,
        UNSUPPORTED,
        JNI_UNAVAILABLE
    }

    private final EMITRustPhase phase;
    private final Status status;
    private final long durationMs;
    private final String message;
    private final Throwable throwable;

    private EMITRustPhaseResult(EMITRustPhase phase, Status status, long durationMs, String message, Throwable throwable)
    {
        this.phase = phase;
        this.status = status;
        this.durationMs = durationMs;
        this.message = message;
        this.throwable = throwable;
    }

    public EMITRustPhase getPhase()
    {
        return this.phase;
    }

    public Status getStatus()
    {
        return this.status;
    }

    public long getDurationMs()
    {
        return this.durationMs;
    }

    public String getMessage()
    {
        return this.message;
    }

    public Throwable getThrowable()
    {
        return this.throwable;
    }

    public boolean isSuccess()
    {
        return this.status == Status.SUCCESS || this.status == Status.SKIPPED || this.status == Status.UNSUPPORTED;
    }

    public boolean isFailure()
    {
        return this.status == Status.FAILURE;
    }

    public static EMITRustPhaseResult success(EMITRustPhase phase, long durationMs, String message)
    {
        return new EMITRustPhaseResult(phase, Status.SUCCESS, durationMs, message, null);
    }

    public static EMITRustPhaseResult failure(EMITRustPhase phase, long durationMs, String message, Throwable throwable)
    {
        return new EMITRustPhaseResult(phase, Status.FAILURE, durationMs, message, throwable);
    }

    public static EMITRustPhaseResult skipped(EMITRustPhase phase, String reason)
    {
        return new EMITRustPhaseResult(phase, Status.SKIPPED, 0L, reason, null);
    }

    public static EMITRustPhaseResult unsupported(EMITRustPhase phase)
    {
        return new EMITRustPhaseResult(phase, Status.UNSUPPORTED, 0L, "no Rust counterpart yet", null);
    }

    public static EMITRustPhaseResult jniUnavailable(EMITRustPhase phase, String reason)
    {
        return new EMITRustPhaseResult(phase, Status.JNI_UNAVAILABLE, 0L, reason, null);
    }
}
