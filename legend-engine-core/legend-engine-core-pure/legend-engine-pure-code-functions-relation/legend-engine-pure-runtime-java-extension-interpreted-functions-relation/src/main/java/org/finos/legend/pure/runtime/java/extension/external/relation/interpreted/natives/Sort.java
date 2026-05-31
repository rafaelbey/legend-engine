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
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.Column;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.RelationType;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.type.generics.GenericType;
import org.finos.legend.pure.m3.exception.PureExecutionException;
import org.finos.legend.pure.m3.navigation.Instance;
import org.finos.legend.pure.m3.navigation.M3Properties;
import org.finos.legend.pure.m3.navigation.ProcessorSupport;
import org.finos.legend.pure.m3.navigation.ValueSpecificationBootstrap;
import org.finos.legend.pure.m3.navigation.relation._Column;
import org.finos.legend.pure.m4.coreinstance.CoreInstance;
import org.finos.legend.pure.m4.ModelRepository;
import org.finos.legend.pure.runtime.java.extension.external.relation.interpreted.natives.shared.Shared;
import org.finos.legend.pure.runtime.java.extension.external.relation.shared.Rows;
import org.finos.legend.pure.runtime.java.extension.external.relation.shared.window.SortDirection;
import org.finos.legend.pure.runtime.java.extension.external.relation.shared.window.SortInfo;
import org.finos.legend.pure.runtime.java.interpreted.ExecutionSupport;
import org.finos.legend.pure.runtime.java.interpreted.FunctionExecutionInterpreted;
import org.finos.legend.pure.runtime.java.interpreted.VariableContext;
import org.finos.legend.pure.runtime.java.interpreted.natives.InstantiationContext;
import org.finos.legend.pure.runtime.java.interpreted.profiler.Profiler;

// Rows-direct: sort the input's TDSTuple rows by cell values at the requested
// columns. Cell values are converted to Comparable Java values via
// Rows.toComparable using the column's Pure type. Nulls sort last (mirrors
// TestTDS.sort's Comparators.safeNullsHigh behaviour).
public class Sort extends Shared
{
    public Sort(FunctionExecutionInterpreted functionExecution, ModelRepository repository)
    {
        super(functionExecution, repository);
    }

    @Override
    public CoreInstance execute(ListIterable<? extends CoreInstance> params, Stack<MutableMap<String, CoreInstance>> resolvedTypeParameters, Stack<MutableMap<String, CoreInstance>> resolvedMultiplicityParameters, VariableContext variableContext, MutableStack<CoreInstance> functionExpressionCallStack, Profiler profiler, InstantiationContext instantiationContext, ExecutionSupport executionSupport, Context context, ProcessorSupport processorSupport) throws PureExecutionException
    {
        CoreInstance tdsInstance = inputAsTDS(params, 0, processorSupport);
        ListIterable<SortInfo> sortInfos = getSortInfos(Instance.getValueForMetaPropertyToManyResolved(params.get(1), M3Properties.values, processorSupport), processorSupport);
        RelationType<?> relType = Rows.relationTypeOf(tdsInstance);
        ListIterable<? extends Column<?, ?>> cols = relType._columns().toList();
        MutableList<CoreInstance> rows = Lists.mutable.<CoreInstance>withAll(Rows.rowsOf(tdsInstance));

        int[] sortIdx = new int[sortInfos.size()];
        GenericType[] sortColType = new GenericType[sortInfos.size()];
        int[] sortDirSign = new int[sortInfos.size()];
        for (int s = 0; s < sortInfos.size(); s++)
        {
            SortInfo si = sortInfos.get(s);
            int idx = -1;
            for (int c = 0; c < cols.size(); c++)
            {
                if (si.columnName.equals(cols.get(c)._name()))
                {
                    idx = c;
                    break;
                }
            }
            if (idx < 0)
            {
                throw new RuntimeException("Sort column '" + si.columnName + "' not found");
            }
            sortIdx[s] = idx;
            sortColType[s] = _Column.getColumnType(cols.get(idx));
            sortDirSign[s] = si.direction == SortDirection.DESC ? -1 : 1;
        }

        rows.sortThis((a, b) ->
        {
            for (int s = 0; s < sortIdx.length; s++)
            {
                @SuppressWarnings("rawtypes")
                Comparable av = Rows.toComparable(Rows.cellAt(a, sortIdx[s]), sortColType[s]);
                @SuppressWarnings("rawtypes")
                Comparable bv = Rows.toComparable(Rows.cellAt(b, sortIdx[s]), sortColType[s]);
                int cmp;
                if (av == null && bv == null)
                {
                    cmp = 0;
                }
                else if (av == null)
                {
                    cmp = 1; // nulls high (last)
                }
                else if (bv == null)
                {
                    cmp = -1;
                }
                else
                {
                    @SuppressWarnings("unchecked")
                    int c0 = av.compareTo(bv);
                    cmp = c0;
                }
                if (cmp != 0)
                {
                    return cmp * sortDirSign[s];
                }
            }
            return 0;
        });

        return ValueSpecificationBootstrap.wrapValueSpecification(Rows.newTDS(Rows.classifierGenericTypeOf(tdsInstance), rows, processorSupport), false, processorSupport);
    }

    public static ListIterable<SortInfo> getSortInfos(ListIterable<? extends CoreInstance> sortInfo, ProcessorSupport processorSupport)
    {
        return sortInfo.collect(c ->
        {
            String name = c.getValueForMetaPropertyToOne("column").getValueForMetaPropertyToOne("name").getName();
            SortDirection direction = SortDirection.valueOf(c.getValueForMetaPropertyToOne("direction").getName());
            return new SortInfo(name, direction);
        });
    }
}
