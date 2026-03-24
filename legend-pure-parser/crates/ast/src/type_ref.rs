use crate::SourceInfo;

/// Represents a package path like `meta::pure::mapping`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagePath {
    pub path: Vec<String>,
    pub source_info: SourceInfo,
}

impl Default for PackagePath {
    fn default() -> Self {
        Self {
            path: Vec::new(),
            source_info: SourceInfo::dummy(),
        }
    }
}

/// Represents a type reference (PackageableType, RelationType, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Packageable(PackageableType),
    // Future: RelationType, UnitType, etc.
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageableType {
    pub full_path: String,
    pub source_info: SourceInfo,
}

/// Represents a generic type with optional type arguments and multiplicity
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericType {
    pub raw_type: Type,
    pub type_arguments: Vec<GenericType>,
    pub multiplicity_arguments: Vec<Multiplicity>,
}

/// Represents a multiplicity like `[1]`, `[0..1]`, `[*]`, `[1..*]`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Multiplicity {
    pub lower_bound: usize,
    pub upper_bound: Option<usize>, // None = '*'
}

impl Multiplicity {
    pub fn new(lower: usize, upper: Option<usize>) -> Self {
        Self {
            lower_bound: lower,
            upper_bound: upper,
        }
    }

    pub fn pure_one() -> Self {
        Self::new(1, Some(1))
    }
    pub fn pure_zero_one() -> Self {
        Self::new(0, Some(1))
    }
    pub fn pure_many() -> Self {
        Self::new(0, None)
    }
    pub fn pure_one_many() -> Self {
        Self::new(1, None)
    }
}
