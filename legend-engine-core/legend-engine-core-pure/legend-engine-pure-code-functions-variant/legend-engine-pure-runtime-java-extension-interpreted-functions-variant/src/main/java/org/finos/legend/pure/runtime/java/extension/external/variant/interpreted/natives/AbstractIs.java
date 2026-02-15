// Copyright 2025 Goldman Sachs
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


package org.finos.legend.pure.runtime.java.extension.external.variant.interpreted.natives;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.JsonNodeType;
import org.eclipse.collections.api.RichIterable;
import org.eclipse.collections.api.list.ListIterable;
import org.eclipse.collections.api.map.MutableMap;
import org.eclipse.collections.api.stack.MutableStack;
import org.eclipse.collections.impl.factory.Lists;
import org.eclipse.collections.impl.utility.Iterate;
import org.finos.legend.pure.m3.compiler.Context;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.function.FunctionCoreInstanceWrapper;
import org.finos.legend.pure.m3.exception.PureExecutionException;
import org.finos.legend.pure.m3.navigation.*;
import org.finos.legend.pure.m3.navigation.PackageableElement.PackageableElement;
import org.finos.legend.pure.m3.navigation.generictype.GenericType;
import org.finos.legend.pure.m4.ModelRepository;
import org.finos.legend.pure.m4.coreinstance.CoreInstance;
import org.finos.legend.pure.runtime.java.extension.external.variant.VariantInstanceImpl;
import org.finos.legend.pure.runtime.java.interpreted.ExecutionSupport;
import org.finos.legend.pure.runtime.java.interpreted.FunctionExecutionInterpreted;
import org.finos.legend.pure.runtime.java.interpreted.VariableContext;
import org.finos.legend.pure.runtime.java.interpreted.natives.InstantiationContext;
import org.finos.legend.pure.runtime.java.interpreted.natives.MapCoreInstance;
import org.finos.legend.pure.runtime.java.interpreted.natives.NativeFunction;
import org.finos.legend.pure.runtime.java.interpreted.profiler.Profiler;
import org.finos.legend.pure.runtime.java.interpreted.profiler.VoidProfiler;

import java.util.Iterator;
import java.util.Map;
import java.util.Objects;
import java.util.Stack;

public abstract class AbstractIs extends NativeFunction
{
    private final FunctionExecutionInterpreted exec;
    private final ModelRepository repository;
    private final JsonNodeType jsonNodeType;

    public AbstractIs(FunctionExecutionInterpreted exec, ModelRepository repository, JsonNodeType jsonNodeType)
    {
        this.exec = exec;
        this.repository = repository;
        this.jsonNodeType = jsonNodeType;
    }

    @Override
    public CoreInstance execute(ListIterable<? extends CoreInstance> params, Stack<MutableMap<String, CoreInstance>> resolvedTypeParameters, Stack<MutableMap<String, CoreInstance>> resolvedMultiplicityParameters, VariableContext variableContext, MutableStack<CoreInstance> functionExpressionCallStack, Profiler profiler, InstantiationContext instantiationContext, ExecutionSupport executionSupport, Context context, ProcessorSupport processorSupport) throws PureExecutionException
    {
        CoreInstance variantCoreInstance = params.get(0);
        VariantInstanceImpl variant = (VariantInstanceImpl) Instance.getValueForMetaPropertyToManyResolved(variantCoreInstance, M3Properties.values, processorSupport).getAny();
        boolean isResult = false;
        if (variant != null)
        {
            isResult = variant.getJsonNode().getNodeType() == this.jsonNodeType;
        }

        return ValueSpecificationBootstrap.wrapValueSpecification(this.repository.newBooleanCoreInstance(isResult), true, processorSupport);
    }
}
