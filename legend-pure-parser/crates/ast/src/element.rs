use crate::{Expression, Multiplicity, PackagePath, SourceInfo, Type};
use smol_str::SmolStr;
use std::any::Any;

/// Top-level Pure element (Class, Enum, Function, etc.)
pub enum Element {
    // Built-in (###Pure)
    Class(ClassDef),
    Enumeration(EnumDef),
    Function(FunctionDef),
    Profile(ProfileDef),
    Association(AssociationDef),
    Measure(MeasureDef),

    // Extension elements — produced by SectionPlugins (e.g., ###Connection)
    Extension(ExtensionElement),
}

// todo all pkg elements definition should reuse traits to capture pkg, name
// todo tag/stereotypes should be under an AnnotatedElement trait
// todo all pkg elements should also have an import aware trait
// todo define an import definition
// todo PackagePath should be Package, each pointing to the parent package
// todo each component should have source info, including name

/// Represents a Pure Class definition
pub struct ClassDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub super_types: Vec<Type>,
    pub properties: Vec<Property>,
    pub qualified_properties: Vec<QualifiedProperty>,
    pub constraints: Vec<Constraint>,
    pub stereotypes: Vec<StereotypePtr>,
    pub tagged_values: Vec<TaggedValue>,
    pub source_info: SourceInfo,
}

/// Represents a property on a class
pub struct Property {
    pub name: SmolStr,
    pub property_type: Type,
    pub multiplicity: Multiplicity,
    pub stereotypes: Vec<StereotypePtr>,
    pub tagged_values: Vec<TaggedValue>,
    pub source_info: SourceInfo,
}

/// Represents a qualified property (e.g. `fullName() { $this.first + $this.last }`)
pub struct QualifiedProperty {
    pub name: SmolStr,
    pub return_type: Type,
    pub multiplicity: Multiplicity,
    pub parameters: Vec<Variable>,
    pub expression: Vec<Expression>,
    pub stereotypes: Vec<StereotypePtr>,
    pub tagged_values: Vec<TaggedValue>,
    pub source_info: SourceInfo,
}

pub struct Constraint {
    pub name: SmolStr,
    pub function_definition: Expression, // The constraint lambda
    pub source_info: SourceInfo,
}

pub struct EnumDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub values: Vec<EnumValue>,
    pub stereotypes: Vec<StereotypePtr>,
    pub tagged_values: Vec<TaggedValue>,
    pub source_info: SourceInfo,
}

pub struct EnumValue {
    pub value: SmolStr,
    pub stereotypes: Vec<StereotypePtr>,
    pub tagged_values: Vec<TaggedValue>,
    pub source_info: SourceInfo,
}

// Stubs for remaining builtin elements
pub struct FunctionDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub return_type: Type,
    pub return_multiplicity: Multiplicity,
    pub parameters: Vec<Variable>,
    pub expression: Vec<Expression>,
    pub source_info: SourceInfo,
}

pub struct ProfileDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub tags: Vec<ProfileTag>,
    pub stereotypes: Vec<ProfileStereotype>,
    pub source_info: SourceInfo,
}

pub struct ProfileTag {
    pub value: String,
    pub source_info: SourceInfo,
}

pub struct ProfileStereotype {
    pub value: String,
    pub source_info: SourceInfo,
}

pub struct AssociationDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub source_info: SourceInfo,
    // todo missing some details here?
}

pub struct MeasureDef {
    pub package: PackagePath,
    pub name: SmolStr,
    pub source_info: SourceInfo,
}

pub struct Variable {
    pub name: SmolStr,
    // todo should be TypeAndMultiplicity together - if one is set, so is the other...
    pub variable_type: Option<Type>,
    pub multiplicity: Option<Multiplicity>,
    pub source_info: SourceInfo,
}

pub struct StereotypePtr {
    pub profile: String,
    pub value: SmolStr,
    pub profile_source_info: SourceInfo,
    pub source_info: SourceInfo,
}

pub struct TaggedValue {
    pub profile: String,
    pub profile_source_info: SourceInfo,
    pub tag: SmolStr,
    pub value: String,
    pub source_info: SourceInfo,
}

/// Opaque container for plugin-produced elements (from sections like ###Connection)
pub struct ExtensionElement {
    pub section: String,      // e.g., "Connection"
    pub element_type: String, // e.g., "RelationalDatabaseConnection"
    pub package: PackagePath,
    pub name: SmolStr,
    pub data: Box<dyn Any + Send + Sync>, // Plugin-specific data
    pub source_info: SourceInfo,
}
