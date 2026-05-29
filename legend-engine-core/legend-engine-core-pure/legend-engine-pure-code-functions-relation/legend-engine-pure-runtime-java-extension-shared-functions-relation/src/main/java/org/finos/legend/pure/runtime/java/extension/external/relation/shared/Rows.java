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

package org.finos.legend.pure.runtime.java.extension.external.relation.shared;

import org.eclipse.collections.api.factory.Lists;
import org.eclipse.collections.api.list.ListIterable;
import org.eclipse.collections.api.list.MutableList;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.Column;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.relation.RelationType;
import org.finos.legend.pure.m3.coreinstance.meta.pure.metamodel.type.generics.GenericType;
import org.finos.legend.pure.m3.navigation.M3Paths;
import org.finos.legend.pure.m3.navigation.M3Properties;
import org.finos.legend.pure.m3.navigation.ProcessorSupport;
import org.finos.legend.pure.m3.navigation.relation._RelationType;
import org.finos.legend.pure.m4.coreinstance.CoreInstance;

// Construction + read helpers for the post-csv-removal TDS shape (rows : T[*]
// of TDSTuple, where each TDSTuple has values: List<Any>[*]; empty inner list
// = absent cell, one element = present). Mirrors legend-pure's
// `TDSExtension.parse` (write side) and `TDSExtension.renderCsv` (read side),
// but exposes them as primitives the engine relation natives can call instead
// of going through CSV.
//
// Operates on raw CoreInstance to avoid pulling the dsl-tds Java interfaces
// (TDS, TDSTuple) into the shared module's classpath.
public final class Rows
{
    private static final String TDS_PATH = "meta::pure::metamodel::relation::TDS";
    private static final String TDS_TUPLE_PATH = "meta::pure::metamodel::relation::TDSTuple";

    private Rows()
    {
    }

    public static ListIterable<? extends CoreInstance> rowsOf(CoreInstance tdsInstance)
    {
        return tdsInstance.getValueForMetaPropertyToMany("rows");
    }

    public static RelationType<?> relationTypeOf(CoreInstance tdsInstance)
    {
        CoreInstance cgt = tdsInstance.getValueForMetaPropertyToOne(M3Properties.classifierGenericType);
        return (RelationType<?>) ((GenericType) cgt)._typeArguments().getFirst()._rawType();
    }

    public static CoreInstance classifierGenericTypeOf(CoreInstance tdsInstance)
    {
        return tdsInstance.getValueForMetaPropertyToOne(M3Properties.classifierGenericType);
    }

    // Read a TDSTuple row's cell value at the given column index.
    // Returns null for absent cells (holder inner list is empty).
    public static CoreInstance cellAt(CoreInstance row, int colIdx)
    {
        ListIterable<? extends CoreInstance> holders = row.getValueForMetaPropertyToMany("values");
        return holders.get(colIdx).getValueForMetaPropertyToOne("values");
    }

    // Build a single TDSTuple row from cell values (parallel to the RelationType's
    // columns; null entries become empty holders). Overrides
    // classifierGenericType to the RelationType so Pure-level type checks and
    // legend-pure's `$row.colName` hook resolve it as a row of T.
    public static CoreInstance newRow(MutableList<? extends CoreInstance> cellValues, RelationType<?> relType, ProcessorSupport ps)
    {
        MutableList<CoreInstance> holders = Lists.mutable.withInitialCapacity(cellValues.size());
        for (CoreInstance value : cellValues)
        {
            CoreInstance holder = ps.newAnonymousCoreInstance(null, M3Paths.List);
            if (value != null)
            {
                holder.setKeyValues(Lists.mutable.with("values"), Lists.mutable.with(value));
            }
            holders.add(holder);
        }
        CoreInstance row = ps.newAnonymousCoreInstance(null, TDS_TUPLE_PATH);
        row.setKeyValues(Lists.mutable.with("values"), holders);
        CoreInstance rowCgt = ps.newAnonymousCoreInstance(null, M3Paths.GenericType);
        rowCgt.setKeyValues(Lists.mutable.with(M3Properties.rawType), Lists.mutable.with((CoreInstance) relType));
        row.setKeyValues(Lists.mutable.with(M3Properties.classifierGenericType), Lists.mutable.with(rowCgt));
        return row;
    }

    // Build a TDS<T> CoreInstance from an existing classifierGenericType
    // (TDS<RelationType<…>>) and a row collection.
    public static CoreInstance newTDS(CoreInstance classifierGenericType, MutableList<? extends CoreInstance> rows, ProcessorSupport ps)
    {
        CoreInstance tds = ps.newAnonymousCoreInstance(null, TDS_PATH);
        tds.setKeyValues(Lists.mutable.with(M3Properties.classifierGenericType), Lists.mutable.with(classifierGenericType));
        tds.setKeyValues(Lists.mutable.with("rows"), rows);
        return tds;
    }

    // Build a TDS<RelationType<…>> classifierGenericType for a schema described
    // as an ordered list of columns.
    @SuppressWarnings({"unchecked", "rawtypes"})
    public static CoreInstance newTDSClassifierGenericType(ListIterable<? extends Column<?, ?>> columns, ProcessorSupport ps)
    {
        RelationType<?> relType = _RelationType.build((ListIterable) columns, null, ps);
        return tdsClassifierGenericTypeFor(relType, ps);
    }

    public static CoreInstance tdsClassifierGenericTypeFor(RelationType<?> relType, ProcessorSupport ps)
    {
        CoreInstance tdsType = ps.package_getByUserPath(TDS_PATH);
        CoreInstance typeArg = ps.newAnonymousCoreInstance(null, M3Paths.GenericType);
        typeArg.setKeyValues(Lists.mutable.with(M3Properties.rawType), Lists.mutable.with((CoreInstance) relType));
        CoreInstance cgt = ps.newAnonymousCoreInstance(null, M3Paths.GenericType);
        cgt.setKeyValues(Lists.mutable.with(M3Properties.rawType), Lists.mutable.with(tdsType));
        cgt.setKeyValues(Lists.mutable.with(M3Properties.typeArguments), Lists.mutable.with(typeArg));
        return cgt;
    }

    // Convenience: read all cells of one column from a row collection. Useful for
    // sort / distinct / groupBy operations that need column-major access; avoids
    // the column-major store TestTDS used to provide.
    public static MutableList<CoreInstance> columnValues(ListIterable<? extends CoreInstance> rows, int colIdx)
    {
        MutableList<CoreInstance> out = Lists.mutable.withInitialCapacity(rows.size());
        for (CoreInstance row : rows)
        {
            out.add(cellAt(row, colIdx));
        }
        return out;
    }
}
