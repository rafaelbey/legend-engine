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

package org.finos.legend.pure.runtime.java.extension.external.relation.interpreted.natives;

import java.util.Stack;
import org.eclipse.collections.api.list.ListIterable;
import org.eclipse.collections.api.list.MutableList;
import org.eclipse.collections.api.map.MutableMap;
import org.eclipse.collections.api.stack.MutableStack;
import org.eclipse.collections.impl.factory.Lists;
import org.finos.legend.pure.m3.compiler.Context;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.ColSpec;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.ColSpecArray;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.Column;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.RelationType;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.type.generics.GenericType;
import org.finos.legend.pure.m3.exception.PureExecutionException;
import org.finos.legend.pure.m3.navigation.Instance;
import org.finos.legend.pure.m3.navigation.M3Properties;
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

// Rows-direct: project each TDSTuple's cells to just the requested columns
// (or pass through unchanged when no col spec is provided). New rows carry
// the projected RelationType taken from returnGenericType so legend-pure's
// $row.colName hook resolves against the narrowed schema.
public class Select extends Shared
{
    public Select(FunctionExecutionInterpreted functionExecution, ModelRepository repository)
    {
        super(functionExecution, repository);
    }

    @Override
    public CoreInstance execute(ListIterable<? extends CoreInstance> params, Stack<MutableMap<String, CoreInstance>> resolvedTypeParameters, Stack<MutableMap<String, CoreInstance>> resolvedMultiplicityParameters, VariableContext variableContext, MutableStack<CoreInstance> functionExpressionCallStack, Profiler profiler, InstantiationContext instantiationContext, ExecutionSupport executionSupport, Context context, ProcessorSupport processorSupport) throws PureExecutionException
    {
        CoreInstance returnGenericType = getReturnGenericType(resolvedTypeParameters, resolvedMultiplicityParameters, functionExpressionCallStack, processorSupport);
        CoreInstance tdsInstance = inputAsTDS(params, 0, processorSupport);
        RelationType<?> inputRelType = Rows.relationTypeOf(tdsInstance);
        ListIterable<? extends Column<?, ?>> inputCols = inputRelType._columns().toList();

        MutableList<String> ids;
        if (params.size() == 1)
        {
            ids = inputCols.collect(Column::_name).toList();
        }
        else
        {
            Object cols = Instance.getValueForMetaPropertyToOneResolved(params.get(1), M3Properties.values, processorSupport);
            if (cols instanceof ColSpec)
            {
                ids = Lists.mutable.with(((ColSpec<?>) cols)._name());
            }
            else if (cols instanceof ColSpecArray)
            {
                ids = Lists.mutable.<String>withAll(((ColSpecArray<?>) cols)._names());
            }
            else
            {
                throw new RuntimeException("Not Possible");
            }
        }

        int[] selectedIdx = new int[ids.size()];
        for (int s = 0; s < ids.size(); s++)
        {
            String name = ids.get(s);
            int idx = -1;
            for (int c = 0; c < inputCols.size(); c++)
            {
                if (name.equals(inputCols.get(c)._name()))
                {
                    idx = c;
                    break;
                }
            }
            if (idx < 0)
            {
                throw new RuntimeException("Column '" + name + "' not found in input relation");
            }
            selectedIdx[s] = idx;
        }

        RelationType<?> outRelType = (RelationType<?>) ((GenericType) returnGenericType)._typeArguments().getFirst()._rawType();
        MutableList<CoreInstance> outRows = Lists.mutable.empty();
        for (CoreInstance row : Rows.rowsOf(tdsInstance))
        {
            MutableList<CoreInstance> projected = Lists.mutable.withInitialCapacity(selectedIdx.length);
            for (int i = 0; i < selectedIdx.length; i++)
            {
                projected.add(Rows.cellAt(row, selectedIdx[i]));
            }
            outRows.add(Rows.newRow(projected, outRelType, processorSupport));
        }
        return ValueSpecificationBootstrap.wrapValueSpecification(Rows.newTDS(returnGenericType, outRows, processorSupport), false, processorSupport);
    }
}
