//! Implementation of permutation argument.

use crate::{
    arithmetic::CurveAffine,
    helpers::{
        polynomial_slice_byte_length, read_polynomial_vec, write_polynomial_slice,
        SerdeCurveAffine, SerdePrimeField,
    },
    plonk::Error,
    poly::{Coeff, ExtendedLagrangeCoeff, LagrangeCoeff, Polynomial},
    SerdeFormat,
};
use halo2_middleware::circuit::{Any, Column};
use halo2_middleware::permutation::{ArgumentV2, Cell};

use std::io;

pub mod keygen;

/// A permutation argument.
#[derive(Debug, Clone)]
pub struct Argument {
    /// A sequence of columns involved in the argument.
    pub(super) columns: Vec<Column<Any>>,
}

impl From<ArgumentV2> for Argument {
    fn from(arg: ArgumentV2) -> Self {
        Self {
            columns: arg.columns.clone(),
        }
    }
}

impl Argument {
    pub(crate) fn new() -> Self {
        Argument { columns: vec![] }
    }

    /// Returns the minimum circuit degree required by the permutation argument.
    /// The argument may use larger degree gates depending on the actual
    /// circuit's degree and how many columns are involved in the permutation.
    pub(crate) fn required_degree(&self) -> usize {
        // degree 2:
        // l_0(X) * (1 - z(X)) = 0
        //
        // We will fit as many polynomials p_i(X) as possible
        // into the required degree of the circuit, so the
        // following will not affect the required degree of
        // this middleware.
        //
        // (1 - (l_last(X) + l_blind(X))) * (
        //   z(\omega X) \prod (p(X) + \beta s_i(X) + \gamma)
        // - z(X) \prod (p(X) + \delta^i \beta X + \gamma)
        // )
        //
        // On the first sets of columns, except the first
        // set, we will do
        //
        // l_0(X) * (z(X) - z'(\omega^(last) X)) = 0
        //
        // where z'(X) is the permutation for the previous set
        // of columns.
        //
        // On the final set of columns, we will do
        //
        // degree 3:
        // l_last(X) * (z'(X)^2 - z'(X)) = 0
        //
        // which will allow the last value to be zero to
        // ensure the argument is perfectly complete.

        // There are constraints of degree 3 regardless of the
        // number of columns involved.
        3
    }

    pub(crate) fn add_column(&mut self, column: Column<Any>) {
        if !self.columns.contains(&column) {
            self.columns.push(column);
        }
    }

    /// Returns columns that participate on the permutation argument.
    pub fn get_columns(&self) -> Vec<Column<Any>> {
        self.columns.clone()
    }
}

/// The verifying key for a single permutation argument.
#[derive(Clone, Debug)]
pub struct VerifyingKey<C: CurveAffine> {
    commitments: Vec<C>,
}

impl<C: CurveAffine> VerifyingKey<C> {
    /// Returns commitments of sigma polynomials
    pub fn commitments(&self) -> &Vec<C> {
        &self.commitments
    }

    pub(crate) fn write<W: io::Write>(&self, writer: &mut W, format: SerdeFormat) -> io::Result<()>
    where
        C: SerdeCurveAffine,
    {
        for commitment in &self.commitments {
            commitment.write(writer, format)?;
        }
        Ok(())
    }

    pub(crate) fn read<R: io::Read>(
        reader: &mut R,
        argument: &Argument,
        format: SerdeFormat,
    ) -> io::Result<Self>
    where
        C: SerdeCurveAffine,
    {
        let commitments = (0..argument.columns.len())
            .map(|_| C::read(reader, format))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(VerifyingKey { commitments })
    }

    pub(crate) fn bytes_length(&self, format: SerdeFormat) -> usize
    where
        C: SerdeCurveAffine,
    {
        self.commitments.len() * C::byte_length(format)
    }
}

/// The proving key for a single permutation argument.
#[derive(Clone, Debug)]
pub(crate) struct ProvingKey<C: CurveAffine> {
    permutations: Vec<Polynomial<C::Scalar, LagrangeCoeff>>,
    polys: Vec<Polynomial<C::Scalar, Coeff>>,
    pub(super) cosets: Vec<Polynomial<C::Scalar, ExtendedLagrangeCoeff>>,
}

impl<C: SerdeCurveAffine> ProvingKey<C>
where
    C::Scalar: SerdePrimeField,
{
    /// Reads proving key for a single permutation argument from buffer using `Polynomial::read`.  
    pub(super) fn read<R: io::Read>(reader: &mut R, format: SerdeFormat) -> io::Result<Self> {
        let permutations = read_polynomial_vec(reader, format)?;
        let polys = read_polynomial_vec(reader, format)?;
        let cosets = read_polynomial_vec(reader, format)?;
        Ok(ProvingKey {
            permutations,
            polys,
            cosets,
        })
    }

    /// Writes proving key for a single permutation argument to buffer using `Polynomial::write`.  
    pub(super) fn write<W: io::Write>(
        &self,
        writer: &mut W,
        format: SerdeFormat,
    ) -> io::Result<()> {
        write_polynomial_slice(&self.permutations, writer, format)?;
        write_polynomial_slice(&self.polys, writer, format)?;
        write_polynomial_slice(&self.cosets, writer, format)?;
        Ok(())
    }
}

impl<C: CurveAffine> ProvingKey<C> {
    /// Gets the total number of bytes in the serialization of `self`
    pub(super) fn bytes_length(&self) -> usize {
        polynomial_slice_byte_length(&self.permutations)
            + polynomial_slice_byte_length(&self.polys)
            + polynomial_slice_byte_length(&self.cosets)
    }
}

// TODO: Move to frontend
#[derive(Clone, Debug)]
pub struct AssemblyFront {
    n: usize,
    columns: Vec<Column<Any>>,
    pub(crate) copies: Vec<(Cell, Cell)>,
}

impl AssemblyFront {
    pub(crate) fn new(n: usize, p: &Argument) -> Self {
        Self {
            n,
            columns: p.columns.clone(),
            copies: Vec::new(),
        }
    }

    pub(crate) fn copy(
        &mut self,
        left_column: Column<Any>,
        left_row: usize,
        right_column: Column<Any>,
        right_row: usize,
    ) -> Result<(), Error> {
        if !self.columns.contains(&left_column) {
            return Err(Error::ColumnNotInPermutation(left_column));
        }
        if !self.columns.contains(&right_column) {
            return Err(Error::ColumnNotInPermutation(right_column));
        }
        // Check bounds
        if left_row >= self.n || right_row >= self.n {
            return Err(Error::BoundsFailure);
        }
        self.copies.push((
            Cell {
                column: left_column,
                row: left_row,
            },
            Cell {
                column: right_column,
                row: right_row,
            },
        ));
        Ok(())
    }
}