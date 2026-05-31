// Copyright 2023 Goldman Sachs
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

package org.finos.legend.pure.runtime.java.extension.external.relation.interpreted.natives;

import java.util.Stack;
import org.eclipse.collections.api.list.FixedSizeList;
import org.eclipse.collections.api.list.ListIterable;
import org.eclipse.collections.api.list.MutableList;
import org.eclipse.collections.api.map.MutableMap;
import org.eclipse.collections.api.stack.MutableStack;
import org.eclipse.collections.impl.factory.Lists;
import org.finos.legend.pure.m3.compiler.Context;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.function.LambdaFunction;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.function.LambdaFunctionCoreInstanceWrapper;
import org.finos.legend.pure.m3.exception.PureExecutionException;
import org.finos.legend.pure.m3.navigation.Instance;
import org.finos.legend.pure.m3.navigation.M3Properties;
import org.finos.legend.pure.m3.navigation.PrimitiveUtilities;
import org.finos.legend.pure.m3.navigation.ProcessorSupport;
import org.finos.legend.pure.m3.navigation.ValueSpecificationBootstrap;
import org.finos.legend.pure.m4.ModelRepository;
import org.finos.legend.pure.m4.coreinstance.CoreInstance;
import org.finos.legend.pure.runtime.java.extension.external.relation.interpreted.natives.shared.Shared;
import org.finos.legend.pure.runtime.java.extension.external.relation.shared.Rows;
import org.finos.legend.pure.runtime.java.interpreted.ExecutionSupport;
import org.finos.legend.pure.runtime.java.interpreted.FunctionExecutionInterpreted;
import org.finos.legend.pure.runtime.java.interpreted.VariableContext;
import org.finos.legend.pure.runtime.java.interpreted.natives.InstantiationContext;
import org.finos.legend.pure.runtime.java.interpreted.profiler.Profiler;

// Rows-direct implementation: iterate the input TDS's `rows : T[*]` of TDSTuples,
// invoke the predicate lambda with each row (legend-pure's
// TDSExtensionInterpreted hook resolves `$row.colName` from the TDSTuple
// classifierGenericType override), and emit a new-shape TDS over the kept rows.
// No TestTDS / TDSWithCursorCoreInstance / TDSCoreInstance involved.
public class Filter extends Shared
{
    public Filter(FunctionExecutionInterpreted functionExecution, ModelRepository repository)
    {
        super(functionExecution, repository);
    }

    @Override
    public CoreInstance execute(ListIterable<? extends CoreInstance> params, Stack<MutableMap<String, CoreInstance>> resolvedTypeParameters, Stack<MutableMap<String, CoreInstance>> resolvedMultiplicityParameters, VariableContext variableContext, MutableStack<CoreInstance> functionExpressionCallStack, Profiler profiler, InstantiationContext instantiationContext, ExecutionSupport executionSupport, Context context, ProcessorSupport processorSupport) throws PureExecutionException
    {
        CoreInstance tdsInstance = inputAsTDS(params, 0, processorSupport);
        ListIterable<? extends CoreInstance> rows = Rows.rowsOf(tdsInstance);
        CoreInstance classifierGT = Rows.classifierGenericTypeOf(tdsInstance);

        CoreInstance filterFunction = Instance.getValueForMetaPropertyToOneResolved(params.get(1), M3Properties.values, processorSupport);
        LambdaFunction<CoreInstance> lambdaFunction = (LambdaFunction<CoreInstance>) LambdaFunctionCoreInstanceWrapper.toLambdaFunction(filterFunction);
        VariableContext evalVarContext = this.getParentOrEmptyVariableContextForLambda(variableContext, filterFunction);

        MutableList<CoreInstance> keptRows = Lists.mutable.empty();
        FixedSizeList<CoreInstance> parameters = Lists.fixedSize.with((CoreInstance) null);
        for (CoreInstance row : rows)
        {
            parameters.set(0, ValueSpecificationBootstrap.wrapValueSpecification(row, true, processorSupport));
            CoreInstance subResult = this.functionExecution.executeFunction(false, lambdaFunction, parameters, resolvedTypeParameters, resolvedMultiplicityParameters, evalVarContext, functionExpressionCallStack, profiler, instantiationContext, executionSupport);
            if (PrimitiveUtilities.getBooleanValue(Instance.getValueForMetaPropertyToOneResolved(subResult, M3Properties.values, processorSupport)))
            {
                keptRows.add(row);
            }
        }
        return ValueSpecificationBootstrap.wrapValueSpecification(Rows.newTDS(classifierGT, keptRows, processorSupport), false, processorSupport);
    }
}
