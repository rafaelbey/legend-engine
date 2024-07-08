// Copyright 2024 Goldman Sachs
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
//

package org.finos.legend.engine.server.test.benchmark;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ForkJoinPool;
import org.apache.commons.io.IOUtils;
import org.finos.legend.engine.language.pure.compiler.Compiler;
import org.finos.legend.engine.language.pure.compiler.toPureGraph.PureModel;
import org.finos.legend.engine.language.pure.compiler.toPureGraph.PureModelProcessParameter;
import org.finos.legend.engine.language.pure.grammar.from.PureGrammarParser;
import org.finos.legend.engine.protocol.pure.v1.model.context.PureModelContextData;
import org.finos.legend.engine.shared.core.deployment.DeploymentMode;
import org.openjdk.jmh.annotations.Benchmark;
import org.openjdk.jmh.annotations.BenchmarkMode;
import org.openjdk.jmh.annotations.Fork;
import org.openjdk.jmh.annotations.Mode;
import org.openjdk.jmh.annotations.Param;
import org.openjdk.jmh.annotations.Scope;
import org.openjdk.jmh.annotations.Setup;
import org.openjdk.jmh.annotations.State;
import org.openjdk.jmh.annotations.TearDown;
import org.openjdk.jmh.annotations.Warmup;

public class PureModelBenchmark
{
    @Benchmark
    @Fork(
            value = 2,
            jvmArgsPrepend = {
                    "-Dlogback.configurationFile=logback.xml",
                    // todo this needs to be pass directly to VM
                    "-Djmh.separateClasspathJAR=true"
            }
    )
    @Warmup(iterations = 3)
    @BenchmarkMode(value = {Mode.AverageTime})
    public PureModel compile(CompilerBenchmarkInput input)
    {
        return Compiler.compile(input.pmcd, DeploymentMode.PROD, "anonymous", null, input.processParameter);
    }

    @State(Scope.Benchmark)
    public static class CompilerBenchmarkInput
    {
        private PureModelContextData pmcd;
        @Param({"1", "2", "4", "8", "16", "64", "128"})
        private int pmcdCount;
        @Param({"1"})
        private int parallelism;

        private transient PureModelProcessParameter processParameter;

        @Setup
        public void setup() throws IOException
        {
            String modelTemplate = IOUtils.toString(ClassLoader.getSystemResource("benchmark.pure"), StandardCharsets.UTF_8);
            StringBuilder model = new StringBuilder(modelTemplate);

            for (int i = 2; i <= this.pmcdCount; i++)
            {
                model.append(modelTemplate.replace("showcase::northwind", String.format("showcase%d::northwind%d", i, i)));
            }

            this.pmcd = PureGrammarParser.newInstance().parseModel(model.toString());

            this.processParameter = PureModelProcessParameter.newBuilder()
                    .withForkJoinPool(new ForkJoinPool(this.parallelism))
                    .build();
        }

        @TearDown
        public void tearDown()
        {
            System.out.println("\n\n" + this.processParameter.getForkJoinPool() + "\n\n");
            this.processParameter.getForkJoinPool().shutdown();
        }
    }

    public static void main(String... args) throws IOException
    {
        CompilerBenchmarkInput compilerBenchmarkInput = new CompilerBenchmarkInput();
        compilerBenchmarkInput.parallelism = 1;
        compilerBenchmarkInput.pmcdCount = 1;
        compilerBenchmarkInput.setup();

        PureModelBenchmark pureModelBenchmark = new PureModelBenchmark();

        while (true)
        {
            long time = System.currentTimeMillis();
            pureModelBenchmark.compile(compilerBenchmarkInput);
            System.out.println("Time: " + (System.currentTimeMillis() - time) + "ms");
        }
    }
}
