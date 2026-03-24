use legend_pure_parser_ast::{ClassInstance, SourceInfo};
use legend_pure_parser_parser::{IslandPlugin, ParseError, ISLAND_PLUGINS};
use linkme::distributed_slice;
use std::any::Any;

pub struct RelationAccessorPlugin;

pub struct RelationAccessorData {
    pub path: Vec<String>,
}

impl IslandPlugin for RelationAccessorPlugin {
    fn island_type(&self) -> &str {
        ">" // #>{}#
    }

    fn parse(&self, content: &str, source_info: SourceInfo) -> Result<ClassInstance, ParseError> {
        let path = content.split('.').map(|s| s.to_string()).collect();
        Ok(ClassInstance {
            instance_type: "relationAccessor".to_string(),
            data: Box::new(RelationAccessorData { path }),
            source_info,
        })
    }
}

// Automatically register this plugin!
#[distributed_slice(ISLAND_PLUGINS)]
fn register() -> Box<dyn IslandPlugin> {
    Box::new(RelationAccessorPlugin)
}
