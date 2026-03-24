use crate::{SourceInfo, Type, Variable};
use smol_str::SmolStr;
use std::any::Any;

/// A ValueSpecification in Pure
pub enum Expression {
    Variable(Variable),
    Lambda(Lambda),
    Application(FunctionApplication),
    Property(PropertyAccess),
    Literal(Literal),
    Collection(Vec<Expression>),
    ColumnSpec(ColumnSpec),

    // Extension hook: Island grammar results land here (#>{}#, #s{}#, etc.)
    ClassInstance(ClassInstance),

    // Advanced constructs
    Let(LetExpression),
    Not(Box<Expression>),
    ArithmeticOp {
        op: ArithOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    BooleanOp {
        op: BoolOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    ComparisonOp {
        op: CompOp,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

pub struct Lambda {
    pub parameters: Vec<Variable>,
    pub body: Vec<Expression>,
    pub source_info: SourceInfo,
}

pub struct FunctionApplication {
    pub function_name: SmolStr,
    pub parameters: Vec<Expression>,
    pub source_info: SourceInfo,
}

pub struct PropertyAccess {
    // Left-hand side object might be implicit (e.g. in qualified property bodies)
    // but in raw AST we capture what we parse (`$p.name` vs `name()`)
    pub property_name: SmolStr,
    pub target: Option<Box<Expression>>,
    pub source_info: SourceInfo,
}

pub enum Literal {
    String(String, SourceInfo),
    Integer(i64, SourceInfo),
    Float(f64, SourceInfo),
    Decimal(String, SourceInfo), // Store as string to preserve precision
    Boolean(bool, SourceInfo),
    Date(String, SourceInfo), // Store exact date string
}

pub struct LetExpression {
    pub name: SmolStr,
    pub value: Box<Expression>,
    pub source_info: SourceInfo,
}

pub struct ColumnSpec {
    pub name: SmolStr,
    pub col_type: Option<Type>,
    pub mapping_function: Option<Lambda>,
    pub source_info: SourceInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithOp {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompOp {
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
}

/// Opaque container for island-grammar-produced values (from #>{}#, #s{}#, etc.)
pub struct ClassInstance {
    pub instance_type: String,            // e.g., "relationalStoreAccessor"
    pub data: Box<dyn Any + Send + Sync>, // Plugin-specific data
    pub source_info: SourceInfo,
}
