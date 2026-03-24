#[cfg(test)]
mod tests {
    use legend_pure_parser_ast::*;
    use smol_str::SmolStr;
    use crate::Emitter;

    #[test]
    fn test_emit_class_def() {
        let class_def = Element::Class(ClassDef {
            package: PackagePath {
                path: vec!["model".to_string(), "domain".to_string()],
                source_info: SourceInfo::dummy(),
            },
            name: SmolStr::new("Person"),
            super_types: vec![],
            properties: vec![
                Property {
                    name: SmolStr::new("firstName"),
                    property_type: Type::Packageable(PackageableType {
                        full_path: "String".to_string(),
                        source_info: SourceInfo::dummy(),
                    }),
                    multiplicity: Multiplicity::pure_one(),
                    stereotypes: vec![],
                    tagged_values: vec![],
                    source_info: SourceInfo::new("test.pure", 2, 5, 2, 20),
                }
            ],
            qualified_properties: vec![],
            constraints: vec![],
            stereotypes: vec![],
            tagged_values: vec![],
            source_info: SourceInfo::new("test.pure", 1, 1, 3, 1),
        });

        let emitter = Emitter::new();
        let json = emitter.emit_element(&class_def).unwrap();

        assert_eq!(json["_type"], "class");
        assert_eq!(json["name"], "Person");
        assert_eq!(json["package"], "model::domain");
        assert_eq!(json["sourceInformation"]["sourceId"], "test.pure");
        assert_eq!(json["sourceInformation"]["startLine"], 1);

        let props = json["properties"].as_array().expect("properties must be an array");
        assert_eq!(props.len(), 1);
        assert_eq!(props[0]["name"], "firstName");
        assert_eq!(props[0]["type"], "String");
        assert_eq!(props[0]["multiplicity"]["lowerBound"], 1);
        assert_eq!(props[0]["multiplicity"]["upperBound"], 1);
        assert_eq!(props[0]["sourceInformation"]["startLine"], 2);
    }
}
