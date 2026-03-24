use legend_pure_parser_ast::*;
use serde_json::{json, Value};
use std::any::Any;
use thiserror::Error;

#[cfg(test)]
mod tests;

#[derive(Error, Debug)]
pub enum EmitError {
    #[error("Failed to serialize to JSON")]
    SerializationError(#[from] serde_json::Error),
    #[error("Unknown extension item type: {0}")]
    UnknownExtensionType(String),
}

/// The base trait for a plugin that knows how to emit its `ClassInstance` payload to JSON
pub trait EmitPlugin: Send + Sync {
    fn emit_type(&self) -> &str;
    fn emit(&self, data: &dyn Any) -> Result<Value, EmitError>;
}

/// Emits the Layer 0 AST to Protocol JSON representation
pub struct Emitter;

impl Emitter {
    pub fn new() -> Self {
        Self
    }

    pub fn emit_elements(&self, elements: &[Element]) -> Result<Vec<Value>, EmitError> {
        let mut result = Vec::with_capacity(elements.len());
        for element in elements {
            result.push(self.emit_element(element)?);
        }
        Ok(result)
    }

    pub fn emit_element(&self, element: &Element) -> Result<Value, EmitError> {
        match element {
            Element::Class(c) => {
                let mut properties_json = Vec::new();
                for prop in &c.properties {
                    properties_json.push(json!({
                        "name": prop.name.as_str(),
                        "type": match &prop.property_type {
                            Type::Packageable(p) => p.full_path.clone(),
                            _ => "Unknown".to_string()
                        },
                        "multiplicity": {
                            "lowerBound": prop.multiplicity.lower_bound,
                            "upperBound": prop.multiplicity.upper_bound,
                        },
                        "sourceInformation": self.emit_source_info(&prop.source_info)
                    }));
                }

                let super_types: Vec<Value> = c.super_types.iter().map(|st| match st {
                    Type::Packageable(p) => json!({
                        "type": "CLASS",
                        "path": p.full_path,
                        "sourceInformation": self.emit_source_info(&p.source_info)
                    }),
                    _ => json!({})
                }).collect();

                Ok(json!({
                    "_type": "class",
                    "name": c.name.as_str(),
                    "package": c.package.path.join("::"),
                    "superTypes": super_types,
                    "properties": properties_json,
                    "stereotypes": self.emit_stereotypes(&c.stereotypes),
                    "taggedValues": self.emit_tagged_values(&c.tagged_values),
                    "sourceInformation": self.emit_source_info(&c.source_info)
                }))
            },
            Element::Profile(p) => {
                let tags: Vec<Value> = p.tags.iter().map(|t| json!({
                    "value": t.value,
                    "sourceInformation": self.emit_source_info(&t.source_info)
                })).collect();
                let stereotypes: Vec<Value> = p.stereotypes.iter().map(|t| json!({
                    "value": t.value,
                    "sourceInformation": self.emit_source_info(&t.source_info)
                })).collect();

                Ok(json!({
                    "_type": "profile",
                    "name": p.name.as_str(),
                    "package": p.package.path.join("::"),
                    "stereotypes": stereotypes,
                    "tags": tags,
                    "sourceInformation": self.emit_source_info(&p.source_info)
                }))
            },
            Element::Extension(ext) => {
                Ok(json!({
                    "_type": ext.element_type,
                    "package": ext.package.path.join("::"),
                    "name": ext.name.as_str()
                }))
            }
            _ => Ok(json!({ "_type": "unknown_element" }))
        }
    }

    fn emit_source_info(&self, src: &SourceInfo) -> Value {
        if src.source_id.is_empty() {
            return Value::Null;
        }
        let end_col = if src.end_column > 0 { src.end_column - 1 } else { src.end_column };
        json!({
            "sourceId": src.source_id,
            "startLine": src.start_line,
            "startColumn": src.start_column,
            "endLine": src.end_line,
            "endColumn": end_col.max(src.start_column)
        })
    }

    fn emit_stereotypes(&self, stereos: &[StereotypePtr]) -> Vec<Value> {
        stereos.iter().map(|s| json!({
            "profile": s.profile,
            "value": s.value.as_str()
        })).collect()
    }

    fn emit_tagged_values(&self, tags: &[TaggedValue]) -> Vec<Value> {
        tags.iter().map(|tv| json!({
            "tag": { "profile": tv.profile.clone(), "value": tv.tag.as_str() },
            "value": tv.value.clone()
        })).collect()
    }
}
