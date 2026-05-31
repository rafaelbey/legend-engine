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
import org.eclipse.collections.api.list.ListIterable;
import org.eclipse.collections.api.list.MutableList;
import org.eclipse.collections.api.map.MutableMap;
import org.eclipse.collections.api.stack.MutableStack;
import org.eclipse.collections.impl.factory.Lists;
import org.finos.legend.pure.m3.compiler.Context;
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

// Rows-direct: keep cells in the same positions and rebuild each row with the
// renamed RelationType classifier override so legend-pure's `$row.colName`
// hook resolves the new column name. Cell positions are unchanged because the
// Pure-side return type preserves column ordering with only the name swapped.
public class Rename extends Shared
{
    public Rename(FunctionExecutionInterpreted functionExecution, ModelRepository repository)
    {
        super(functionExecution, repository);
    }

    @Override
    public CoreInstance execute(ListIterable<? extends CoreInstance> params, Stack<MutableMap<String, CoreInstance>> resolvedTypeParameters, Stack<MutableMap<String, CoreInstance>> resolvedMultiplicityParameters, VariableContext variableContext, MutableStack<CoreInstance> functionExpressionCallStack, Profiler profiler, InstantiationContext instantiationContext, ExecutionSupport executionSupport, Context context, ProcessorSupport processorSupport) throws PureExecutionException
    {
        CoreInstance returnGenericType = getReturnGenericType(resolvedTypeParameters, resolvedMultiplicityParameters, functionExpressionCallStack, processorSupport);
        CoreInstance tdsInstance = inputAsTDS(params, 0, processorSupport);
        // params 1 and 2 (old/new ColSpec) are reflected in returnGenericType already;
        // we just need the new RelationType to thread through Rows.newRow.
        Instance.getValueForMetaPropertyToOneResolved(params.get(1), M3Properties.values, processorSupport); // old (unused here)
        Instance.getValueForMetaPropertyToOneResolved(params.get(2), M3Properties.values, processorSupport); // new (unused here)
        RelationType<?> outRelType = (RelationType<?>) ((GenericType) returnGenericType)._typeArguments().getFirst()._rawType();
        int colCount = outRelType._columns().size();
        MutableList<CoreInstance> outRows = Lists.mutable.empty();
        for (CoreInstance row : Rows.rowsOf(tdsInstance))
        {
            MutableList<CoreInstance> cells = Lists.mutable.withInitialCapacity(colCount);
            for (int c = 0; c < colCount; c++)
            {
                cells.add(Rows.cellAt(row, c));
            }
            outRows.add(Rows.newRow(cells, outRelType, processorSupport));
        }
        return ValueSpecificationBootstrap.wrapValueSpecification(Rows.newTDS(returnGenericType, outRows, processorSupport), false, processorSupport);
    }
}
