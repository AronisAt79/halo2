use super::{lookup, permutation, shuffle, Queries};
use crate::dev::metadata;
use crate::poly::Rotation;
use core::cmp::max;
use core::ops::{Add, Mul};
use ff::Field;
use sealed::SealedPhase;
use std::collections::HashMap;
use std::fmt::Debug;
use std::iter::{Product, Sum};
use std::{
    convert::TryFrom,
    ops::{Neg, Sub},
};

/// A column type
pub trait ColumnType:
    'static + Sized + Copy + std::fmt::Debug + PartialEq + Eq + Into<Any>
{
    /// Return expression from cell
    fn query_cell<F: Field>(&self, index: usize, at: Rotation) -> Expression<F>;
}

/// A column with an index and type
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Column<C: ColumnType> {
    index: usize,
    column_type: C,
}

impl<C: ColumnType> Column<C> {
    pub(crate) fn new(index: usize, column_type: C) -> Self {
        Column { index, column_type }
    }

    /// Index of this column.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Type of this column.
    pub fn column_type(&self) -> &C {
        &self.column_type
    }

    /// Return expression from column at a relative position
    pub fn query_cell<F: Field>(&self, at: Rotation) -> Expression<F> {
        self.column_type.query_cell(self.index, at)
    }

    /// Return expression from column at the current row
    pub fn cur<F: Field>(&self) -> Expression<F> {
        self.query_cell(Rotation::cur())
    }

    /// Return expression from column at the next row
    pub fn next<F: Field>(&self) -> Expression<F> {
        self.query_cell(Rotation::next())
    }

    /// Return expression from column at the previous row
    pub fn prev<F: Field>(&self) -> Expression<F> {
        self.query_cell(Rotation::prev())
    }

    /// Return expression from column at the specified rotation
    pub fn rot<F: Field>(&self, rotation: i32) -> Expression<F> {
        self.query_cell(Rotation(rotation))
    }
}

impl<C: ColumnType> Ord for Column<C> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // This ordering is consensus-critical! The layouters rely on deterministic column
        // orderings.
        match self.column_type.into().cmp(&other.column_type.into()) {
            // Indices are assigned within column types.
            std::cmp::Ordering::Equal => self.index.cmp(&other.index),
            order => order,
        }
    }
}

impl<C: ColumnType> PartialOrd for Column<C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub(crate) mod sealed {
    /// Phase of advice column
    #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct Phase(pub(crate) u8);

    impl Phase {
        pub fn prev(&self) -> Option<Phase> {
            self.0.checked_sub(1).map(Phase)
        }
    }

    impl SealedPhase for Phase {
        fn to_sealed(self) -> Phase {
            self
        }
    }

    /// Sealed trait to help keep `Phase` private.
    pub trait SealedPhase {
        fn to_sealed(self) -> Phase;
    }
}

/// Phase of advice column
pub trait Phase: SealedPhase {}

impl<P: SealedPhase> Phase for P {}

/// First phase
#[derive(Debug)]
pub struct FirstPhase;

impl SealedPhase for super::FirstPhase {
    fn to_sealed(self) -> sealed::Phase {
        sealed::Phase(0)
    }
}

/// Second phase
#[derive(Debug)]
pub struct SecondPhase;

impl SealedPhase for super::SecondPhase {
    fn to_sealed(self) -> sealed::Phase {
        sealed::Phase(1)
    }
}

/// Third phase
#[derive(Debug)]
pub struct ThirdPhase;

impl SealedPhase for super::ThirdPhase {
    fn to_sealed(self) -> sealed::Phase {
        sealed::Phase(2)
    }
}

/// An advice column
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct Advice {
    pub(crate) phase: sealed::Phase,
}

impl Default for Advice {
    fn default() -> Advice {
        Advice {
            phase: FirstPhase.to_sealed(),
        }
    }
}

impl Advice {
    /// Returns `Advice` in given `Phase`
    pub fn new<P: Phase>(phase: P) -> Advice {
        Advice {
            phase: phase.to_sealed(),
        }
    }

    /// Phase of this column
    pub fn phase(&self) -> u8 {
        self.phase.0
    }
}

impl std::fmt::Debug for Advice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("Advice");
        // Only show advice's phase if it's not in first phase.
        if self.phase != FirstPhase.to_sealed() {
            debug_struct.field("phase", &self.phase);
        }
        debug_struct.finish()
    }
}

/// A fixed column
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Fixed;

/// An instance column
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Instance;

/// An enum over the Advice, Fixed, Instance structs
#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub enum Any {
    /// An Advice variant
    Advice(Advice),
    /// A Fixed variant
    Fixed,
    /// An Instance variant
    Instance,
}

impl Any {
    /// Returns Advice variant in `FirstPhase`
    pub fn advice() -> Any {
        Any::Advice(Advice::default())
    }

    /// Returns Advice variant in given `Phase`
    pub fn advice_in<P: Phase>(phase: P) -> Any {
        Any::Advice(Advice::new(phase))
    }
}

impl std::fmt::Debug for Any {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Any::Advice(advice) => {
                let mut debug_struct = f.debug_struct("Advice");
                // Only show advice's phase if it's not in first phase.
                if advice.phase != FirstPhase.to_sealed() {
                    debug_struct.field("phase", &advice.phase);
                }
                debug_struct.finish()
            }
            Any::Fixed => f.debug_struct("Fixed").finish(),
            Any::Instance => f.debug_struct("Instance").finish(),
        }
    }
}

impl Ord for Any {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // This ordering is consensus-critical! The layouters rely on deterministic column
        // orderings.
        match (self, other) {
            (Any::Instance, Any::Instance) | (Any::Fixed, Any::Fixed) => std::cmp::Ordering::Equal,
            (Any::Advice(lhs), Any::Advice(rhs)) => lhs.phase.cmp(&rhs.phase),
            // Across column types, sort Instance < Advice < Fixed.
            (Any::Instance, Any::Advice(_))
            | (Any::Advice(_), Any::Fixed)
            | (Any::Instance, Any::Fixed) => std::cmp::Ordering::Less,
            (Any::Fixed, Any::Instance)
            | (Any::Fixed, Any::Advice(_))
            | (Any::Advice(_), Any::Instance) => std::cmp::Ordering::Greater,
        }
    }
}

impl PartialOrd for Any {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl ColumnType for Advice {
    fn query_cell<F: Field>(&self, index: usize, at: Rotation) -> Expression<F> {
        Expression::Advice(AdviceQuery {
            index: None,
            column_index: index,
            rotation: at,
            phase: self.phase,
        })
    }
}
impl ColumnType for Fixed {
    fn query_cell<F: Field>(&self, index: usize, at: Rotation) -> Expression<F> {
        Expression::Fixed(FixedQuery {
            index: None,
            column_index: index,
            rotation: at,
        })
    }
}
impl ColumnType for Instance {
    fn query_cell<F: Field>(&self, index: usize, at: Rotation) -> Expression<F> {
        Expression::Instance(InstanceQuery {
            index: None,
            column_index: index,
            rotation: at,
        })
    }
}
impl ColumnType for Any {
    fn query_cell<F: Field>(&self, index: usize, at: Rotation) -> Expression<F> {
        match self {
            Any::Advice(Advice { phase }) => Expression::Advice(AdviceQuery {
                index: None,
                column_index: index,
                rotation: at,
                phase: *phase,
            }),
            Any::Fixed => Expression::Fixed(FixedQuery {
                index: None,
                column_index: index,
                rotation: at,
            }),
            Any::Instance => Expression::Instance(InstanceQuery {
                index: None,
                column_index: index,
                rotation: at,
            }),
        }
    }
}

impl From<Advice> for Any {
    fn from(advice: Advice) -> Any {
        Any::Advice(advice)
    }
}

impl From<Fixed> for Any {
    fn from(_: Fixed) -> Any {
        Any::Fixed
    }
}

impl From<Instance> for Any {
    fn from(_: Instance) -> Any {
        Any::Instance
    }
}

impl From<Column<Advice>> for Column<Any> {
    fn from(advice: Column<Advice>) -> Column<Any> {
        Column {
            index: advice.index(),
            column_type: Any::Advice(advice.column_type),
        }
    }
}

impl From<Column<Fixed>> for Column<Any> {
    fn from(advice: Column<Fixed>) -> Column<Any> {
        Column {
            index: advice.index(),
            column_type: Any::Fixed,
        }
    }
}

impl From<Column<Instance>> for Column<Any> {
    fn from(advice: Column<Instance>) -> Column<Any> {
        Column {
            index: advice.index(),
            column_type: Any::Instance,
        }
    }
}

impl TryFrom<Column<Any>> for Column<Advice> {
    type Error = &'static str;

    fn try_from(any: Column<Any>) -> Result<Self, Self::Error> {
        match any.column_type() {
            Any::Advice(advice) => Ok(Column {
                index: any.index(),
                column_type: *advice,
            }),
            _ => Err("Cannot convert into Column<Advice>"),
        }
    }
}

impl TryFrom<Column<Any>> for Column<Fixed> {
    type Error = &'static str;

    fn try_from(any: Column<Any>) -> Result<Self, Self::Error> {
        match any.column_type() {
            Any::Fixed => Ok(Column {
                index: any.index(),
                column_type: Fixed,
            }),
            _ => Err("Cannot convert into Column<Fixed>"),
        }
    }
}

impl TryFrom<Column<Any>> for Column<Instance> {
    type Error = &'static str;

    fn try_from(any: Column<Any>) -> Result<Self, Self::Error> {
        match any.column_type() {
            Any::Instance => Ok(Column {
                index: any.index(),
                column_type: Instance,
            }),
            _ => Err("Cannot convert into Column<Instance>"),
        }
    }
}

/// Query of fixed column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FixedQueryMid {
    /// Column index
    pub column_index: usize,
    /// Rotation of this query
    pub rotation: Rotation,
}

/// Query of fixed column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FixedQuery {
    /// Query index
    pub(crate) index: Option<usize>,
    /// Column index
    pub(crate) column_index: usize,
    /// Rotation of this query
    pub(crate) rotation: Rotation,
}

impl FixedQuery {
    /// Column index
    pub fn column_index(&self) -> usize {
        self.column_index
    }

    /// Rotation of this query
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }
}

/// Query of advice column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AdviceQueryMid {
    /// Column index
    pub column_index: usize,
    /// Rotation of this query
    pub rotation: Rotation,
    /// Phase of this advice column
    pub phase: sealed::Phase,
}

/// Query of advice column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AdviceQuery {
    /// Query index
    pub(crate) index: Option<usize>,
    /// Column index
    pub(crate) column_index: usize,
    /// Rotation of this query
    pub(crate) rotation: Rotation,
    /// Phase of this advice column
    pub(crate) phase: sealed::Phase,
}

impl AdviceQuery {
    /// Column index
    pub fn column_index(&self) -> usize {
        self.column_index
    }

    /// Rotation of this query
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Phase of this advice column
    pub fn phase(&self) -> u8 {
        self.phase.0
    }
}

/// Query of instance column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InstanceQueryMid {
    /// Column index
    pub column_index: usize,
    /// Rotation of this query
    pub rotation: Rotation,
}

/// Query of instance column at a certain relative location
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InstanceQuery {
    /// Query index
    pub(crate) index: Option<usize>,
    /// Column index
    pub(crate) column_index: usize,
    /// Rotation of this query
    pub(crate) rotation: Rotation,
}

impl InstanceQuery {
    /// Column index
    pub fn column_index(&self) -> usize {
        self.column_index
    }

    /// Rotation of this query
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }
}

/// A challenge squeezed from transcript after advice columns at the phase have been committed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Challenge {
    index: usize,
    pub(crate) phase: sealed::Phase,
}

impl Challenge {
    /// Index of this challenge.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Phase of this challenge.
    pub fn phase(&self) -> u8 {
        self.phase.0
    }

    /// Return Expression
    pub fn expr<F: Field>(&self) -> Expression<F> {
        Expression::Challenge(*self)
    }
}

/// Low-degree expression representing an identity that must hold over the committed columns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionMid<F> {
    /// This is a constant polynomial
    Constant(F),
    /// This is a fixed column queried at a certain relative location
    Fixed(FixedQueryMid),
    /// This is an advice (witness) column queried at a certain relative location
    Advice(AdviceQueryMid),
    /// This is an instance (external) column queried at a certain relative location
    Instance(InstanceQueryMid),
    /// This is a challenge
    Challenge(Challenge),
    /// This is a negated polynomial
    Negated(Box<ExpressionMid<F>>),
    /// This is the sum of two polynomials
    Sum(Box<ExpressionMid<F>>, Box<ExpressionMid<F>>),
    /// This is the product of two polynomials
    Product(Box<ExpressionMid<F>>, Box<ExpressionMid<F>>),
    /// This is a scaled polynomial
    Scaled(Box<ExpressionMid<F>>, F),
}

impl<F: Field> ExpressionMid<F> {
    /// Compute the degree of this polynomial
    pub fn degree(&self) -> usize {
        use ExpressionMid::*;
        match self {
            Constant(_) => 0,
            Fixed(_) => 1,
            Advice(_) => 1,
            Instance(_) => 1,
            Challenge(_) => 0,
            Negated(poly) => poly.degree(),
            Sum(a, b) => max(a.degree(), b.degree()),
            Product(a, b) => a.degree() + b.degree(),
            Scaled(poly, _) => poly.degree(),
        }
    }
}

/// Low-degree expression representing an identity that must hold over the committed columns.
#[derive(Clone, PartialEq, Eq)]
pub enum Expression<F> {
    /// This is a constant polynomial
    Constant(F),
    /// This is a fixed column queried at a certain relative location
    Fixed(FixedQuery),
    /// This is an advice (witness) column queried at a certain relative location
    Advice(AdviceQuery),
    /// This is an instance (external) column queried at a certain relative location
    Instance(InstanceQuery),
    /// This is a challenge
    Challenge(Challenge),
    /// This is a negated polynomial
    Negated(Box<Expression<F>>),
    /// This is the sum of two polynomials
    Sum(Box<Expression<F>>, Box<Expression<F>>),
    /// This is the product of two polynomials
    Product(Box<Expression<F>>, Box<Expression<F>>),
    /// This is a scaled polynomial
    Scaled(Box<Expression<F>>, F),
}

impl<F> Into<ExpressionMid<F>> for Expression<F> {
    fn into(self) -> ExpressionMid<F> {
        match self {
            Expression::Constant(c) => ExpressionMid::Constant(c),
            Expression::Fixed(FixedQuery {
                column_index,
                rotation,
                ..
            }) => ExpressionMid::Fixed(FixedQueryMid {
                column_index,
                rotation,
            }),
            Expression::Advice(AdviceQuery {
                column_index,
                rotation,
                phase,
                ..
            }) => ExpressionMid::Advice(AdviceQueryMid {
                column_index,
                rotation,
                phase,
            }),
            Expression::Instance(InstanceQuery {
                column_index,
                rotation,
                ..
            }) => ExpressionMid::Instance(InstanceQueryMid {
                column_index,
                rotation,
            }),
            Expression::Challenge(c) => ExpressionMid::Challenge(c),
            Expression::Negated(e) => ExpressionMid::Negated(Box::new((*e).into())),
            Expression::Sum(lhs, rhs) => {
                ExpressionMid::Sum(Box::new((*lhs).into()), Box::new((*rhs).into()))
            }
            Expression::Product(lhs, rhs) => {
                ExpressionMid::Product(Box::new((*lhs).into()), Box::new((*rhs).into()))
            }
            Expression::Scaled(e, c) => ExpressionMid::Scaled(Box::new((*e).into()), c),
        }
    }
}

impl<F: Field> Expression<F> {
    /// Evaluate the polynomial using the provided closures to perform the
    /// operations.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate<T>(
        &self,
        constant: &impl Fn(F) -> T,
        fixed_column: &impl Fn(FixedQuery) -> T,
        advice_column: &impl Fn(AdviceQuery) -> T,
        instance_column: &impl Fn(InstanceQuery) -> T,
        challenge: &impl Fn(Challenge) -> T,
        negated: &impl Fn(T) -> T,
        sum: &impl Fn(T, T) -> T,
        product: &impl Fn(T, T) -> T,
        scaled: &impl Fn(T, F) -> T,
    ) -> T {
        match self {
            Expression::Constant(scalar) => constant(*scalar),
            Expression::Fixed(query) => fixed_column(*query),
            Expression::Advice(query) => advice_column(*query),
            Expression::Instance(query) => instance_column(*query),
            Expression::Challenge(value) => challenge(*value),
            Expression::Negated(a) => {
                let a = a.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                negated(a)
            }
            Expression::Sum(a, b) => {
                let a = a.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                let b = b.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                sum(a, b)
            }
            Expression::Product(a, b) => {
                let a = a.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                let b = b.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                product(a, b)
            }
            Expression::Scaled(a, f) => {
                let a = a.evaluate(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                );
                scaled(a, *f)
            }
        }
    }

    /// Evaluate the polynomial lazily using the provided closures to perform the
    /// operations.
    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_lazy<T: PartialEq>(
        &self,
        constant: &impl Fn(F) -> T,
        fixed_column: &impl Fn(FixedQuery) -> T,
        advice_column: &impl Fn(AdviceQuery) -> T,
        instance_column: &impl Fn(InstanceQuery) -> T,
        challenge: &impl Fn(Challenge) -> T,
        negated: &impl Fn(T) -> T,
        sum: &impl Fn(T, T) -> T,
        product: &impl Fn(T, T) -> T,
        scaled: &impl Fn(T, F) -> T,
        zero: &T,
    ) -> T {
        match self {
            Expression::Constant(scalar) => constant(*scalar),
            Expression::Fixed(query) => fixed_column(*query),
            Expression::Advice(query) => advice_column(*query),
            Expression::Instance(query) => instance_column(*query),
            Expression::Challenge(value) => challenge(*value),
            Expression::Negated(a) => {
                let a = a.evaluate_lazy(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                    zero,
                );
                negated(a)
            }
            Expression::Sum(a, b) => {
                let a = a.evaluate_lazy(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                    zero,
                );
                let b = b.evaluate_lazy(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                    zero,
                );
                sum(a, b)
            }
            Expression::Product(a, b) => {
                let (a, b) = if a.complexity() <= b.complexity() {
                    (a, b)
                } else {
                    (b, a)
                };
                let a = a.evaluate_lazy(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                    zero,
                );

                if a == *zero {
                    a
                } else {
                    let b = b.evaluate_lazy(
                        constant,
                        fixed_column,
                        advice_column,
                        instance_column,
                        challenge,
                        negated,
                        sum,
                        product,
                        scaled,
                        zero,
                    );
                    product(a, b)
                }
            }
            Expression::Scaled(a, f) => {
                let a = a.evaluate_lazy(
                    constant,
                    fixed_column,
                    advice_column,
                    instance_column,
                    challenge,
                    negated,
                    sum,
                    product,
                    scaled,
                    zero,
                );
                scaled(a, *f)
            }
        }
    }

    fn write_identifier<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Expression::Constant(scalar) => write!(writer, "{scalar:?}"),
            Expression::Fixed(query) => {
                write!(
                    writer,
                    "fixed[{}][{}]",
                    query.column_index, query.rotation.0
                )
            }
            Expression::Advice(query) => {
                write!(
                    writer,
                    "advice[{}][{}]",
                    query.column_index, query.rotation.0
                )
            }
            Expression::Instance(query) => {
                write!(
                    writer,
                    "instance[{}][{}]",
                    query.column_index, query.rotation.0
                )
            }
            Expression::Challenge(challenge) => {
                write!(writer, "challenge[{}]", challenge.index())
            }
            Expression::Negated(a) => {
                writer.write_all(b"(-")?;
                a.write_identifier(writer)?;
                writer.write_all(b")")
            }
            Expression::Sum(a, b) => {
                writer.write_all(b"(")?;
                a.write_identifier(writer)?;
                writer.write_all(b"+")?;
                b.write_identifier(writer)?;
                writer.write_all(b")")
            }
            Expression::Product(a, b) => {
                writer.write_all(b"(")?;
                a.write_identifier(writer)?;
                writer.write_all(b"*")?;
                b.write_identifier(writer)?;
                writer.write_all(b")")
            }
            Expression::Scaled(a, f) => {
                a.write_identifier(writer)?;
                write!(writer, "*{f:?}")
            }
        }
    }

    /// Identifier for this expression. Expressions with identical identifiers
    /// do the same calculation (but the expressions don't need to be exactly equal
    /// in how they are composed e.g. `1 + 2` and `2 + 1` can have the same identifier).
    pub fn identifier(&self) -> String {
        let mut cursor = std::io::Cursor::new(Vec::new());
        self.write_identifier(&mut cursor).unwrap();
        String::from_utf8(cursor.into_inner()).unwrap()
    }

    /// Compute the degree of this polynomial
    pub fn degree(&self) -> usize {
        match self {
            Expression::Constant(_) => 0,
            Expression::Fixed(_) => 1,
            Expression::Advice(_) => 1,
            Expression::Instance(_) => 1,
            Expression::Challenge(_) => 0,
            Expression::Negated(poly) => poly.degree(),
            Expression::Sum(a, b) => max(a.degree(), b.degree()),
            Expression::Product(a, b) => a.degree() + b.degree(),
            Expression::Scaled(poly, _) => poly.degree(),
        }
    }

    /// Approximate the computational complexity of this expression.
    pub fn complexity(&self) -> usize {
        match self {
            Expression::Constant(_) => 0,
            Expression::Fixed(_) => 1,
            Expression::Advice(_) => 1,
            Expression::Instance(_) => 1,
            Expression::Challenge(_) => 0,
            Expression::Negated(poly) => poly.complexity() + 5,
            Expression::Sum(a, b) => a.complexity() + b.complexity() + 15,
            Expression::Product(a, b) => a.complexity() + b.complexity() + 30,
            Expression::Scaled(poly, _) => poly.complexity() + 30,
        }
    }

    /// Square this expression.
    pub fn square(self) -> Self {
        self.clone() * self
    }
}

impl<F: std::fmt::Debug> std::fmt::Debug for Expression<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Expression::Constant(scalar) => f.debug_tuple("Constant").field(scalar).finish(),
            // Skip enum variant and print query struct directly to maintain backwards compatibility.
            Expression::Fixed(query) => {
                let mut debug_struct = f.debug_struct("Fixed");
                match query.index {
                    None => debug_struct.field("query_index", &query.index),
                    Some(idx) => debug_struct.field("query_index", &idx),
                };
                debug_struct
                    .field("column_index", &query.column_index)
                    .field("rotation", &query.rotation)
                    .finish()
            }
            Expression::Advice(query) => {
                let mut debug_struct = f.debug_struct("Advice");
                match query.index {
                    None => debug_struct.field("query_index", &query.index),
                    Some(idx) => debug_struct.field("query_index", &idx),
                };
                debug_struct
                    .field("column_index", &query.column_index)
                    .field("rotation", &query.rotation);
                // Only show advice's phase if it's not in first phase.
                if query.phase != FirstPhase.to_sealed() {
                    debug_struct.field("phase", &query.phase);
                }
                debug_struct.finish()
            }
            Expression::Instance(query) => {
                let mut debug_struct = f.debug_struct("Instance");
                match query.index {
                    None => debug_struct.field("query_index", &query.index),
                    Some(idx) => debug_struct.field("query_index", &idx),
                };
                debug_struct
                    .field("column_index", &query.column_index)
                    .field("rotation", &query.rotation)
                    .finish()
            }
            Expression::Challenge(challenge) => {
                f.debug_tuple("Challenge").field(challenge).finish()
            }
            Expression::Negated(poly) => f.debug_tuple("Negated").field(poly).finish(),
            Expression::Sum(a, b) => f.debug_tuple("Sum").field(a).field(b).finish(),
            Expression::Product(a, b) => f.debug_tuple("Product").field(a).field(b).finish(),
            Expression::Scaled(poly, scalar) => {
                f.debug_tuple("Scaled").field(poly).field(scalar).finish()
            }
        }
    }
}

impl<F: Field> Neg for Expression<F> {
    type Output = Expression<F>;
    fn neg(self) -> Self::Output {
        Expression::Negated(Box::new(self))
    }
}

impl<F: Field> Add for Expression<F> {
    type Output = Expression<F>;
    fn add(self, rhs: Expression<F>) -> Expression<F> {
        Expression::Sum(Box::new(self), Box::new(rhs))
    }
}

impl<F: Field> Sub for Expression<F> {
    type Output = Expression<F>;
    fn sub(self, rhs: Expression<F>) -> Expression<F> {
        Expression::Sum(Box::new(self), Box::new(-rhs))
    }
}

impl<F: Field> Mul for Expression<F> {
    type Output = Expression<F>;
    fn mul(self, rhs: Expression<F>) -> Expression<F> {
        Expression::Product(Box::new(self), Box::new(rhs))
    }
}

impl<F: Field> Mul<F> for Expression<F> {
    type Output = Expression<F>;
    fn mul(self, rhs: F) -> Expression<F> {
        Expression::Scaled(Box::new(self), rhs)
    }
}

impl<F: Field> Sum<Self> for Expression<F> {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.reduce(|acc, x| acc + x)
            .unwrap_or(Expression::Constant(F::ZERO))
    }
}

impl<F: Field> Product<Self> for Expression<F> {
    fn product<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.reduce(|acc, x| acc * x)
            .unwrap_or(Expression::Constant(F::ONE))
    }
}

/// Represents an index into a vector where each entry corresponds to a distinct
/// point that polynomials are queried at.
#[derive(Copy, Clone, Debug)]
pub(crate) struct PointIndex(pub usize);

/// A "virtual cell" is a PLONK cell that has been queried at a particular relative offset
/// within a custom gate.
#[derive(Clone, Debug)]
pub struct VirtualCell {
    pub(crate) column: Column<Any>,
    pub(crate) rotation: Rotation,
}

impl<Col: Into<Column<Any>>> From<(Col, Rotation)> for VirtualCell {
    fn from((column, rotation): (Col, Rotation)) -> Self {
        VirtualCell {
            column: column.into(),
            rotation,
        }
    }
}

/// An individual polynomial constraint.
///
/// These are returned by the closures passed to `ConstraintSystem::create_gate`.
#[derive(Debug)]
pub struct Constraint<F: Field> {
    name: String,
    poly: Expression<F>,
}

impl<F: Field> From<Expression<F>> for Constraint<F> {
    fn from(poly: Expression<F>) -> Self {
        Constraint {
            name: "".to_string(),
            poly,
        }
    }
}

impl<F: Field, S: AsRef<str>> From<(S, Expression<F>)> for Constraint<F> {
    fn from((name, poly): (S, Expression<F>)) -> Self {
        Constraint {
            name: name.as_ref().to_string(),
            poly,
        }
    }
}

impl<F: Field> From<Expression<F>> for Vec<Constraint<F>> {
    fn from(poly: Expression<F>) -> Self {
        vec![Constraint {
            name: "".to_string(),
            poly,
        }]
    }
}

/// A set of polynomial constraints with a common selector.
///
/// ```
/// use halo2_backend::{plonk::{Constraints, Expression}, poly::Rotation};
/// use halo2curves::pasta::Fp;
/// # use halo2_backend::plonk::ConstraintSystem;
///
/// # let mut meta = ConstraintSystem::<Fp>::default();
/// let a = meta.advice_column();
/// let b = meta.advice_column();
/// let c = meta.advice_column();
/// let s = meta.selector();
///
/// meta.create_gate("foo", |meta| {
///     let next = meta.query_advice(a, Rotation::next());
///     let a = meta.query_advice(a, Rotation::cur());
///     let b = meta.query_advice(b, Rotation::cur());
///     let c = meta.query_advice(c, Rotation::cur());
///     let s_ternary = meta.query_selector(s);
///
///     let one_minus_a = Expression::Constant(Fp::one()) - a.clone();
///
///     Constraints::with_selector(
///         s_ternary,
///         std::array::IntoIter::new([
///             ("a is boolean", a.clone() * one_minus_a.clone()),
///             ("next == a ? b : c", next - (a * b + one_minus_a * c)),
///         ]),
///     )
/// });
/// ```
///
/// Note that the use of `std::array::IntoIter::new` is only necessary if you need to
/// support Rust 1.51 or 1.52. If your minimum supported Rust version is 1.53 or greater,
/// you can pass an array directly.
#[derive(Debug)]
pub struct Constraints<F: Field, C: Into<Constraint<F>>, Iter: IntoIterator<Item = C>> {
    selector: Expression<F>,
    constraints: Iter,
}

impl<F: Field, C: Into<Constraint<F>>, Iter: IntoIterator<Item = C>> Constraints<F, C, Iter> {
    /// Constructs a set of constraints that are controlled by the given selector.
    ///
    /// Each constraint `c` in `iterator` will be converted into the constraint
    /// `selector * c`.
    pub fn with_selector(selector: Expression<F>, constraints: Iter) -> Self {
        Constraints {
            selector,
            constraints,
        }
    }
}

fn apply_selector_to_constraint<F: Field, C: Into<Constraint<F>>>(
    (selector, c): (Expression<F>, C),
) -> Constraint<F> {
    let constraint: Constraint<F> = c.into();
    Constraint {
        name: constraint.name,
        poly: selector * constraint.poly,
    }
}

type ApplySelectorToConstraint<F, C> = fn((Expression<F>, C)) -> Constraint<F>;
type ConstraintsIterator<F, C, I> = std::iter::Map<
    std::iter::Zip<std::iter::Repeat<Expression<F>>, I>,
    ApplySelectorToConstraint<F, C>,
>;

impl<F: Field, C: Into<Constraint<F>>, Iter: IntoIterator<Item = C>> IntoIterator
    for Constraints<F, C, Iter>
{
    type Item = Constraint<F>;
    type IntoIter = ConstraintsIterator<F, C, Iter::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::repeat(self.selector)
            .zip(self.constraints)
            .map(apply_selector_to_constraint)
    }
}

/// A Gate contains a single polynomial identity with a name as metadata.
#[derive(Clone, Debug)]
pub struct GateV2Backend<F: Field> {
    name: String,
    poly: ExpressionMid<F>,
}

impl<F: Field> GateV2Backend<F> {
    /// Returns the gate name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the polynomial identity of this gate
    pub fn polynomial(&self) -> &ExpressionMid<F> {
        &self.poly
    }
}

/// Gate
#[derive(Clone, Debug)]
pub struct Gate<F: Field> {
    name: String,
    constraint_names: Vec<String>,
    polys: Vec<Expression<F>>,
    /// We track queried selectors separately from other cells, so that we can use them to
    /// trigger debug checks on gates.
    queried_cells: Vec<VirtualCell>,
}

impl<F: Field> Gate<F> {
    /// Returns the gate name.
    pub fn name(&self) -> &str {
        self.name.as_str()
    }

    /// Returns the name of the constraint at index `constraint_index`.
    pub fn constraint_name(&self, constraint_index: usize) -> &str {
        self.constraint_names[constraint_index].as_str()
    }

    /// Returns constraints of this gate
    pub fn polynomials(&self) -> &[Expression<F>] {
        &self.polys
    }
}

/// Data that needs to be preprocessed from a circuit
#[derive(Debug, Clone)]
pub struct PreprocessingV2<F: Field> {
    // TODO(Edu): Can we replace this by a simpler structure?
    pub(crate) permutation: permutation::keygen::Assembly,
    pub(crate) fixed: Vec<Vec<F>>,
}

/// This is a description of a low level Plonkish compiled circuit. Contains the Constraint System
/// as well as the fixed columns and copy constraints information.
#[derive(Debug, Clone)]
pub struct CompiledCircuitV2<F: Field> {
    pub(crate) preprocessing: PreprocessingV2<F>,
    pub(crate) cs: ConstraintSystemV2Backend<F>,
}

struct QueriesMap {
    advice_map: HashMap<(Column<Advice>, Rotation), usize>,
    instance_map: HashMap<(Column<Instance>, Rotation), usize>,
    fixed_map: HashMap<(Column<Fixed>, Rotation), usize>,
    advice: Vec<(Column<Advice>, Rotation)>,
    instance: Vec<(Column<Instance>, Rotation)>,
    fixed: Vec<(Column<Fixed>, Rotation)>,
}

impl QueriesMap {
    fn add_advice(&mut self, col: Column<Advice>, rot: Rotation) -> usize {
        *self.advice_map.entry((col, rot)).or_insert_with(|| {
            self.advice.push((col, rot));
            self.advice.len() - 1
        })
    }
    fn add_instance(&mut self, col: Column<Instance>, rot: Rotation) -> usize {
        *self.instance_map.entry((col, rot)).or_insert_with(|| {
            self.instance.push((col, rot));
            self.instance.len() - 1
        })
    }
    fn add_fixed(&mut self, col: Column<Fixed>, rot: Rotation) -> usize {
        *self.fixed_map.entry((col, rot)).or_insert_with(|| {
            self.fixed.push((col, rot));
            self.fixed.len() - 1
        })
    }
}

impl QueriesMap {
    fn as_expression<F: Field>(&mut self, expr: &ExpressionMid<F>) -> Expression<F> {
        match expr {
            ExpressionMid::Constant(c) => Expression::Constant(*c),
            ExpressionMid::Fixed(query) => {
                let (col, rot) = (Column::new(query.column_index, Fixed), query.rotation);
                let index = self.add_fixed(col, rot);
                Expression::Fixed(FixedQuery {
                    index: Some(index),
                    column_index: query.column_index,
                    rotation: query.rotation,
                })
            }
            ExpressionMid::Advice(query) => {
                let (col, rot) = (
                    Column::new(query.column_index, Advice { phase: query.phase }),
                    query.rotation,
                );
                let index = self.add_advice(col, rot);
                Expression::Advice(AdviceQuery {
                    index: Some(index),
                    column_index: query.column_index,
                    rotation: query.rotation,
                    phase: query.phase,
                })
            }
            ExpressionMid::Instance(query) => {
                let (col, rot) = (Column::new(query.column_index, Instance), query.rotation);
                let index = self.add_instance(col, rot);
                Expression::Instance(InstanceQuery {
                    index: Some(index),
                    column_index: query.column_index,
                    rotation: query.rotation,
                })
            }
            ExpressionMid::Challenge(c) => Expression::Challenge(*c),
            ExpressionMid::Negated(e) => Expression::Negated(Box::new(self.as_expression(e))),
            ExpressionMid::Sum(lhs, rhs) => Expression::Sum(
                Box::new(self.as_expression(lhs)),
                Box::new(self.as_expression(rhs)),
            ),
            ExpressionMid::Product(lhs, rhs) => Expression::Product(
                Box::new(self.as_expression(lhs)),
                Box::new(self.as_expression(rhs)),
            ),
            ExpressionMid::Scaled(e, c) => Expression::Scaled(Box::new(self.as_expression(e)), *c),
        }
    }
}

/// This is a description of the circuit environment, such as the gate, column and
/// permutation arrangements.
#[derive(Debug, Clone)]
pub struct ConstraintSystemV2Backend<F: Field> {
    pub(crate) num_fixed_columns: usize,
    pub(crate) num_advice_columns: usize,
    pub(crate) num_instance_columns: usize,
    pub(crate) num_challenges: usize,

    /// Contains the index of each advice column that is left unblinded.
    pub(crate) unblinded_advice_columns: Vec<usize>,

    /// Contains the phase for each advice column. Should have same length as num_advice_columns.
    pub(crate) advice_column_phase: Vec<u8>,
    /// Contains the phase for each challenge. Should have same length as num_challenges.
    pub(crate) challenge_phase: Vec<u8>,

    pub(crate) gates: Vec<GateV2Backend<F>>,

    // Permutation argument for performing equality constraints
    pub(crate) permutation: permutation::Argument,

    // Vector of lookup arguments, where each corresponds to a sequence of
    // input expressions and a sequence of table expressions involved in the lookup.
    pub(crate) lookups: Vec<lookup::ArgumentV2<F>>,

    // Vector of shuffle arguments, where each corresponds to a sequence of
    // input expressions and a sequence of shuffle expressions involved in the shuffle.
    pub(crate) shuffles: Vec<shuffle::ArgumentV2<F>>,

    // List of indexes of Fixed columns which are associated to a circuit-general Column tied to their annotation.
    pub(crate) general_column_annotations: HashMap<metadata::Column, String>,
}

impl<F: Field> Into<ConstraintSystemV2Backend<F>> for ConstraintSystem<F> {
    fn into(self) -> ConstraintSystemV2Backend<F> {
        ConstraintSystemV2Backend {
            num_fixed_columns: self.num_fixed_columns,
            num_advice_columns: self.num_advice_columns,
            num_instance_columns: self.num_instance_columns,
            num_challenges: self.num_challenges,
            unblinded_advice_columns: self.unblinded_advice_columns.clone(),
            advice_column_phase: self.advice_column_phase.iter().map(|p| p.0).collect(),
            challenge_phase: self.challenge_phase.iter().map(|p| p.0).collect(),
            gates: self
                .gates
                .iter()
                .map(|g| {
                    g.polys.clone().into_iter().enumerate().map(|(i, e)| {
                        let name = match g.constraint_name(i) {
                            "" => g.name.clone(),
                            constraint_name => format!("{}:{}", g.name, constraint_name),
                        };
                        GateV2Backend {
                            name,
                            poly: e.into(),
                        }
                    })
                })
                .flatten()
                .collect(),
            permutation: self.permutation.clone(),
            lookups: self
                .lookups
                .iter()
                .map(|l| lookup::ArgumentV2 {
                    name: l.name.clone(),
                    input_expressions: l
                        .input_expressions
                        .clone()
                        .into_iter()
                        .map(|e| e.into())
                        .collect(),
                    table_expressions: l
                        .table_expressions
                        .clone()
                        .into_iter()
                        .map(|e| e.into())
                        .collect(),
                })
                .collect(),
            shuffles: self
                .shuffles
                .iter()
                .map(|s| shuffle::ArgumentV2 {
                    name: s.name.clone(),
                    input_expressions: s
                        .input_expressions
                        .clone()
                        .into_iter()
                        .map(|e| e.into())
                        .collect(),
                    shuffle_expressions: s
                        .shuffle_expressions
                        .clone()
                        .into_iter()
                        .map(|e| e.into())
                        .collect(),
                })
                .collect(),
            general_column_annotations: self.general_column_annotations.clone(),
        }
    }
}

impl<F: Field> ConstraintSystemV2Backend<F> {
    /// Collect queries used in gates while mapping those gates to equivalent ones with indexed
    /// query references in the expressions.
    fn collect_queries_gates(&self, queries: &mut QueriesMap) -> Vec<Gate<F>> {
        self.gates
            .iter()
            .map(|gate| Gate {
                name: gate.name.clone(),
                constraint_names: Vec::new(),
                polys: vec![queries.as_expression(gate.polynomial())],
                queried_cells: Vec::new(), // Unused?
            })
            .collect()
    }

    /// Collect queries used in lookups while mapping those lookups to equivalent ones with indexed
    /// query references in the expressions.
    fn collect_queries_lookups(&self, queries: &mut QueriesMap) -> Vec<lookup::Argument<F>> {
        self.lookups
            .iter()
            .map(|lookup| lookup::Argument {
                name: lookup.name.clone(),
                input_expressions: lookup
                    .input_expressions
                    .iter()
                    .map(|e| queries.as_expression(e))
                    .collect(),
                table_expressions: lookup
                    .table_expressions
                    .iter()
                    .map(|e| queries.as_expression(e))
                    .collect(),
            })
            .collect()
    }

    /// Collect queries used in shuffles while mapping those lookups to equivalent ones with indexed
    /// query references in the expressions.
    fn collect_queries_shuffles(&self, queries: &mut QueriesMap) -> Vec<shuffle::Argument<F>> {
        self.shuffles
            .iter()
            .map(|shuffle| shuffle::Argument {
                name: shuffle.name.clone(),
                input_expressions: shuffle
                    .input_expressions
                    .iter()
                    .map(|e| queries.as_expression(e))
                    .collect(),
                shuffle_expressions: shuffle
                    .shuffle_expressions
                    .iter()
                    .map(|e| queries.as_expression(e))
                    .collect(),
            })
            .collect()
    }

    /// Collect all queries used in the expressions of gates, lookups and shuffles.  Map the
    /// expressions of gates, lookups and shuffles into equivalent ones with indexed query
    /// references.
    pub(crate) fn collect_queries(
        &self,
    ) -> (
        Queries,
        Vec<Gate<F>>,
        Vec<lookup::Argument<F>>,
        Vec<shuffle::Argument<F>>,
    ) {
        let mut queries = QueriesMap {
            advice_map: HashMap::new(),
            instance_map: HashMap::new(),
            fixed_map: HashMap::new(),
            advice: Vec::new(),
            instance: Vec::new(),
            fixed: Vec::new(),
        };

        let gates = self.collect_queries_gates(&mut queries);
        let lookups = self.collect_queries_lookups(&mut queries);
        let shuffles = self.collect_queries_shuffles(&mut queries);

        // Each column used in a copy constraint involves a query at rotation current.
        for column in self.permutation.get_columns() {
            match column.column_type {
                Any::Instance => {
                    queries.add_instance(Column::new(column.index(), Instance), Rotation::cur())
                }
                Any::Fixed => {
                    queries.add_fixed(Column::new(column.index(), Fixed), Rotation::cur())
                }
                Any::Advice(advice) => {
                    queries.add_advice(Column::new(column.index(), advice), Rotation::cur())
                }
            };
        }

        let mut num_advice_queries = vec![0; self.num_advice_columns];
        for (column, _) in queries.advice.iter() {
            num_advice_queries[column.index()] += 1;
        }

        let queries = Queries {
            advice: queries.advice,
            instance: queries.instance,
            fixed: queries.fixed,
            num_advice_queries,
        };
        (queries, gates, lookups, shuffles)
    }
}

/// This is a description of the circuit environment, such as the gate, column and
/// permutation arrangements.
#[derive(Debug, Clone)]
pub struct ConstraintSystem<F: Field> {
    pub(crate) num_fixed_columns: usize,
    pub(crate) num_advice_columns: usize,
    pub(crate) num_instance_columns: usize,
    pub(crate) num_selectors: usize,
    pub(crate) num_challenges: usize,

    /// Contains the index of each advice column that is left unblinded.
    pub(crate) unblinded_advice_columns: Vec<usize>,

    /// Contains the phase for each advice column. Should have same length as num_advice_columns.
    pub(crate) advice_column_phase: Vec<sealed::Phase>,
    /// Contains the phase for each challenge. Should have same length as num_challenges.
    pub(crate) challenge_phase: Vec<sealed::Phase>,

    pub(crate) gates: Vec<Gate<F>>,
    pub(crate) advice_queries: Vec<(Column<Advice>, Rotation)>,
    // Contains an integer for each advice column
    // identifying how many distinct queries it has
    // so far; should be same length as num_advice_columns.
    pub(crate) num_advice_queries: Vec<usize>,
    pub(crate) instance_queries: Vec<(Column<Instance>, Rotation)>,
    pub(crate) fixed_queries: Vec<(Column<Fixed>, Rotation)>,

    // Permutation argument for performing equality constraints
    pub(crate) permutation: permutation::Argument,

    // Vector of lookup arguments, where each corresponds to a sequence of
    // input expressions and a sequence of table expressions involved in the lookup.
    pub(crate) lookups: Vec<lookup::Argument<F>>,

    // Vector of shuffle arguments, where each corresponds to a sequence of
    // input expressions and a sequence of shuffle expressions involved in the shuffle.
    pub(crate) shuffles: Vec<shuffle::Argument<F>>,

    // List of indexes of Fixed columns which are associated to a circuit-general Column tied to their annotation.
    pub(crate) general_column_annotations: HashMap<metadata::Column, String>,

    // Vector of fixed columns, which can be used to store constant values
    // that are copied into advice columns.
    pub(crate) constants: Vec<Column<Fixed>>,

    pub(crate) minimum_degree: Option<usize>,
}

impl<F: Field> From<ConstraintSystemV2Backend<F>> for ConstraintSystem<F> {
    fn from(cs2: ConstraintSystemV2Backend<F>) -> Self {
        let (queries, gates, lookups, shuffles) = cs2.collect_queries();
        ConstraintSystem {
            num_fixed_columns: cs2.num_fixed_columns,
            num_advice_columns: cs2.num_advice_columns,
            num_instance_columns: cs2.num_instance_columns,
            num_selectors: 0,
            num_challenges: cs2.num_challenges,
            unblinded_advice_columns: cs2.unblinded_advice_columns,
            advice_column_phase: cs2
                .advice_column_phase
                .into_iter()
                .map(sealed::Phase)
                .collect(),
            challenge_phase: cs2.challenge_phase.into_iter().map(sealed::Phase).collect(),
            gates,
            advice_queries: queries.advice,
            num_advice_queries: queries.num_advice_queries,
            instance_queries: queries.instance,
            fixed_queries: queries.fixed,
            permutation: cs2.permutation,
            lookups,
            shuffles,
            general_column_annotations: cs2.general_column_annotations,
            constants: Vec::new(),
            minimum_degree: None,
        }
    }
}

/// Represents the minimal parameters that determine a `ConstraintSystem`.
#[allow(dead_code)]
pub struct PinnedConstraintSystem<'a, F: Field> {
    num_fixed_columns: &'a usize,
    num_advice_columns: &'a usize,
    num_instance_columns: &'a usize,
    num_selectors: &'a usize,
    num_challenges: &'a usize,
    advice_column_phase: &'a Vec<sealed::Phase>,
    challenge_phase: &'a Vec<sealed::Phase>,
    gates: PinnedGates<'a, F>,
    advice_queries: &'a Vec<(Column<Advice>, Rotation)>,
    instance_queries: &'a Vec<(Column<Instance>, Rotation)>,
    fixed_queries: &'a Vec<(Column<Fixed>, Rotation)>,
    permutation: &'a permutation::Argument,
    lookups: &'a Vec<lookup::Argument<F>>,
    shuffles: &'a Vec<shuffle::Argument<F>>,
    constants: &'a Vec<Column<Fixed>>,
    minimum_degree: &'a Option<usize>,
}

impl<'a, F: Field> std::fmt::Debug for PinnedConstraintSystem<'a, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug_struct = f.debug_struct("PinnedConstraintSystem");
        debug_struct
            .field("num_fixed_columns", self.num_fixed_columns)
            .field("num_advice_columns", self.num_advice_columns)
            .field("num_instance_columns", self.num_instance_columns)
            .field("num_selectors", self.num_selectors);
        // Only show multi-phase related fields if it's used.
        if *self.num_challenges > 0 {
            debug_struct
                .field("num_challenges", self.num_challenges)
                .field("advice_column_phase", self.advice_column_phase)
                .field("challenge_phase", self.challenge_phase);
        }
        debug_struct
            .field("gates", &self.gates)
            .field("advice_queries", self.advice_queries)
            .field("instance_queries", self.instance_queries)
            .field("fixed_queries", self.fixed_queries)
            .field("permutation", self.permutation)
            .field("lookups", self.lookups);
        if !self.shuffles.is_empty() {
            debug_struct.field("shuffles", self.shuffles);
        }
        debug_struct
            .field("constants", self.constants)
            .field("minimum_degree", self.minimum_degree);
        debug_struct.finish()
    }
}

struct PinnedGates<'a, F: Field>(&'a Vec<Gate<F>>);

impl<'a, F: Field> std::fmt::Debug for PinnedGates<'a, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_list()
            .entries(self.0.iter().flat_map(|gate| gate.polynomials().iter()))
            .finish()
    }
}

impl<F: Field> Default for ConstraintSystem<F> {
    fn default() -> ConstraintSystem<F> {
        ConstraintSystem {
            num_fixed_columns: 0,
            num_advice_columns: 0,
            num_instance_columns: 0,
            num_selectors: 0,
            num_challenges: 0,
            unblinded_advice_columns: Vec::new(),
            advice_column_phase: Vec::new(),
            challenge_phase: Vec::new(),
            gates: vec![],
            fixed_queries: Vec::new(),
            advice_queries: Vec::new(),
            num_advice_queries: Vec::new(),
            instance_queries: Vec::new(),
            permutation: permutation::Argument::new(),
            lookups: Vec::new(),
            shuffles: Vec::new(),
            general_column_annotations: HashMap::new(),
            constants: vec![],
            minimum_degree: None,
        }
    }
}

impl<F: Field> ConstraintSystem<F> {
    /// Obtain a pinned version of this constraint system; a structure with the
    /// minimal parameters needed to determine the rest of the constraint
    /// system.
    pub fn pinned(&self) -> PinnedConstraintSystem<'_, F> {
        PinnedConstraintSystem {
            num_fixed_columns: &self.num_fixed_columns,
            num_advice_columns: &self.num_advice_columns,
            num_instance_columns: &self.num_instance_columns,
            num_selectors: &self.num_selectors,
            num_challenges: &self.num_challenges,
            advice_column_phase: &self.advice_column_phase,
            challenge_phase: &self.challenge_phase,
            gates: PinnedGates(&self.gates),
            fixed_queries: &self.fixed_queries,
            advice_queries: &self.advice_queries,
            instance_queries: &self.instance_queries,
            permutation: &self.permutation,
            lookups: &self.lookups,
            shuffles: &self.shuffles,
            constants: &self.constants,
            minimum_degree: &self.minimum_degree,
        }
    }

    pub(crate) fn get_advice_query_index(&self, column: Column<Advice>, at: Rotation) -> usize {
        for (index, advice_query) in self.advice_queries.iter().enumerate() {
            if advice_query == &(column, at) {
                return index;
            }
        }

        panic!("get_advice_query_index called for non-existent query");
    }

    pub(crate) fn get_fixed_query_index(&self, column: Column<Fixed>, at: Rotation) -> usize {
        for (index, fixed_query) in self.fixed_queries.iter().enumerate() {
            if fixed_query == &(column, at) {
                return index;
            }
        }

        panic!("get_fixed_query_index called for non-existent query");
    }

    pub(crate) fn get_instance_query_index(&self, column: Column<Instance>, at: Rotation) -> usize {
        for (index, instance_query) in self.instance_queries.iter().enumerate() {
            if instance_query == &(column, at) {
                return index;
            }
        }

        panic!("get_instance_query_index called for non-existent query");
    }

    pub(crate) fn get_any_query_index(&self, column: Column<Any>, at: Rotation) -> usize {
        match column.column_type() {
            Any::Advice(_) => {
                self.get_advice_query_index(Column::<Advice>::try_from(column).unwrap(), at)
            }
            Any::Fixed => {
                self.get_fixed_query_index(Column::<Fixed>::try_from(column).unwrap(), at)
            }
            Any::Instance => {
                self.get_instance_query_index(Column::<Instance>::try_from(column).unwrap(), at)
            }
        }
    }

    /// Returns the list of phases
    pub fn phases(&self) -> impl Iterator<Item = sealed::Phase> {
        let max_phase = self
            .advice_column_phase
            .iter()
            .max()
            .map(|phase| phase.0)
            .unwrap_or_default();
        (0..=max_phase).map(sealed::Phase)
    }

    /// Compute the degree of the constraint system (the maximum degree of all
    /// constraints).
    pub fn degree(&self) -> usize {
        // The permutation argument will serve alongside the gates, so must be
        // accounted for.
        let mut degree = self.permutation.required_degree();

        // The lookup argument also serves alongside the gates and must be accounted
        // for.
        degree = std::cmp::max(
            degree,
            self.lookups
                .iter()
                .map(|l| l.required_degree())
                .max()
                .unwrap_or(1),
        );

        // The lookup argument also serves alongside the gates and must be accounted
        // for.
        degree = std::cmp::max(
            degree,
            self.shuffles
                .iter()
                .map(|l| l.required_degree())
                .max()
                .unwrap_or(1),
        );

        // Account for each gate to ensure our quotient polynomial is the
        // correct degree and that our extended domain is the right size.
        degree = std::cmp::max(
            degree,
            self.gates
                .iter()
                .flat_map(|gate| gate.polynomials().iter().map(|poly| poly.degree()))
                .max()
                .unwrap_or(0),
        );

        std::cmp::max(degree, self.minimum_degree.unwrap_or(1))
    }

    /// Compute the number of blinding factors necessary to perfectly blind
    /// each of the prover's witness polynomials.
    pub fn blinding_factors(&self) -> usize {
        // All of the prover's advice columns are evaluated at no more than
        let factors = *self.num_advice_queries.iter().max().unwrap_or(&1);
        // distinct points during gate checks.

        // - The permutation argument witness polynomials are evaluated at most 3 times.
        // - Each lookup argument has independent witness polynomials, and they are
        //   evaluated at most 2 times.
        let factors = std::cmp::max(3, factors);

        // Each polynomial is evaluated at most an additional time during
        // multiopen (at x_3 to produce q_evals):
        let factors = factors + 1;

        // h(x) is derived by the other evaluations so it does not reveal
        // anything; in fact it does not even appear in the proof.

        // h(x_3) is also not revealed; the verifier only learns a single
        // evaluation of a polynomial in x_1 which has h(x_3) and another random
        // polynomial evaluated at x_3 as coefficients -- this random polynomial
        // is "random_poly" in the vanishing argument.

        // Add an additional blinding factor as a slight defense against
        // off-by-one errors.
        factors + 1
    }

    /// Returns the minimum necessary rows that need to exist in order to
    /// account for e.g. blinding factors.
    pub fn minimum_rows(&self) -> usize {
        self.blinding_factors() // m blinding factors
            + 1 // for l_{-(m + 1)} (l_last)
            + 1 // for l_0 (just for extra breathing room for the permutation
                // argument, to essentially force a separation in the
                // permutation polynomial between the roles of l_last, l_0
                // and the interstitial values.)
            + 1 // for at least one row
    }

    /// Returns number of fixed columns
    pub fn num_fixed_columns(&self) -> usize {
        self.num_fixed_columns
    }

    /// Returns number of advice columns
    pub fn num_advice_columns(&self) -> usize {
        self.num_advice_columns
    }

    /// Returns number of instance columns
    pub fn num_instance_columns(&self) -> usize {
        self.num_instance_columns
    }

    /// Returns number of selectors
    pub fn num_selectors(&self) -> usize {
        self.num_selectors
    }

    /// Returns number of challenges
    pub fn num_challenges(&self) -> usize {
        self.num_challenges
    }

    /// Returns phase of advice columns
    pub fn advice_column_phase(&self) -> Vec<u8> {
        self.advice_column_phase
            .iter()
            .map(|phase| phase.0)
            .collect()
    }

    /// Returns phase of challenges
    pub fn challenge_phase(&self) -> Vec<u8> {
        self.challenge_phase.iter().map(|phase| phase.0).collect()
    }

    /// Returns gates
    pub fn gates(&self) -> &Vec<Gate<F>> {
        &self.gates
    }

    /// Returns general column annotations
    pub fn general_column_annotations(&self) -> &HashMap<metadata::Column, String> {
        &self.general_column_annotations
    }

    /// Returns advice queries
    pub fn advice_queries(&self) -> &Vec<(Column<Advice>, Rotation)> {
        &self.advice_queries
    }

    /// Returns instance queries
    pub fn instance_queries(&self) -> &Vec<(Column<Instance>, Rotation)> {
        &self.instance_queries
    }

    /// Returns fixed queries
    pub fn fixed_queries(&self) -> &Vec<(Column<Fixed>, Rotation)> {
        &self.fixed_queries
    }

    /// Returns permutation argument
    pub fn permutation(&self) -> &permutation::Argument {
        &self.permutation
    }

    /// Returns lookup arguments
    pub fn lookups(&self) -> &Vec<lookup::Argument<F>> {
        &self.lookups
    }

    /// Returns shuffle arguments
    pub fn shuffles(&self) -> &Vec<shuffle::Argument<F>> {
        &self.shuffles
    }

    /// Returns constants
    pub fn constants(&self) -> &Vec<Column<Fixed>> {
        &self.constants
    }
}

#[cfg(test)]
mod tests {
    use super::Expression;
    use halo2curves::bn256::Fr;

    #[test]
    fn iter_sum() {
        let exprs: Vec<Expression<Fr>> = vec![
            Expression::Constant(1.into()),
            Expression::Constant(2.into()),
            Expression::Constant(3.into()),
        ];
        let happened: Expression<Fr> = exprs.into_iter().sum();
        let expected: Expression<Fr> = Expression::Sum(
            Box::new(Expression::Sum(
                Box::new(Expression::Constant(1.into())),
                Box::new(Expression::Constant(2.into())),
            )),
            Box::new(Expression::Constant(3.into())),
        );

        assert_eq!(happened, expected);
    }

    #[test]
    fn iter_product() {
        let exprs: Vec<Expression<Fr>> = vec![
            Expression::Constant(1.into()),
            Expression::Constant(2.into()),
            Expression::Constant(3.into()),
        ];
        let happened: Expression<Fr> = exprs.into_iter().product();
        let expected: Expression<Fr> = Expression::Product(
            Box::new(Expression::Product(
                Box::new(Expression::Constant(1.into())),
                Box::new(Expression::Constant(2.into())),
            )),
            Box::new(Expression::Constant(3.into())),
        );

        assert_eq!(happened, expected);
    }
}