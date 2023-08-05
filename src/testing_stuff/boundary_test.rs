use ff::{PrimeField, Field, BatchInvert, BatchInverter};
use group::{Curve, prime::PrimeCurveAffine, Group};
use num_bigint::{BigUint, RandomBits, BigInt};
use num_traits::{Num, Pow, pow, One};
use halo2::{arithmetic::{self, parallelize, best_fft, FftGroup, eval_polynomial, kate_division, best_multiexp}, poly::{commitment::MSM, ipa::{msm::MSMIPA, commitment::ParamsIPA}}, plonk::{Phase, FirstPhase, SecondPhase, ThirdPhase, Challenge, Circuit, Any}, transcript::{self, ChallengeScalar}, circuit::{floor_planner::V1, Layouter}, dev::MockProver};
use halo2curves::{CurveAffine, bn256, grumpkin::{self, G1Affine}, Coordinates, CurveExt};
use halo2curves::bn256::Fr as F;
use halo2curves::grumpkin::Fr as Fq;
use rand_core::OsRng;
use subtle::CtOption;
use std::{ops::{Shl, Add, Mul, Shr}, cmp, iter::*, fmt::{Display, Formatter}, time::SystemTime, marker::PhantomData};
use rand::{Rng, random};
use crate::{argument_witness_calc::{self, order}, negbase_utils::{self, digit_by_id}, regular_functions_utils::{self, Polynomial, FftPrecomp}};

use halo2::circuit::{AssignedCell, Cell, Region, Value};

use halo2::{
    plonk::{
        Advice, Column, ConstraintSystem, Constraints, Expression, Fixed, Selector, TableColumn, Error,
    },
    poly::Rotation,
};

pub type Grumpkin = grumpkin::G1;

pub type AssignedValue<F> = AssignedCell<F, F>;


#[derive(Clone)]
pub struct ExampleGate <F: PrimeField + Ord> {
    pub a: Column<Advice>,
    pub b: Column<Advice>,
    pub c: Column<Advice>,
    pub s: Column<Fixed>,
    pub(crate) _marker: PhantomData<F>,
}

impl<F: PrimeField + Ord> ExampleGate<F> {
    fn configure(meta: &mut ConstraintSystem<F>) -> Self {
        let a = meta.advice_column();
        let b = meta.advice_column();
        let c = meta.advice_column();
        let s = meta.fixed_column();

        meta.create_gate("gate", |meta|{
            let a0 = meta.query_advice(a, Rotation(0));
            let b0 = meta.query_advice(b, Rotation(0));
            let c0 = meta.query_advice(c, Rotation(0));
            let a1 = meta.query_advice(a, Rotation(1));
            let sel = meta.query_fixed(s, Rotation(0)) + meta.query_fixed(s, Rotation(-1));
            let gate = (a1 - a0 - b0 * c0);
            [sel*gate]
        });
    
        Self{a,b,c,s, _marker : PhantomData}
    }
}

#[derive(Clone)]
pub struct ExampleConfig <F: PrimeField + Ord> {
    pub gate: ExampleGate<F>,
}

pub struct ExampleCircuit <F: PrimeField + Ord> {
    pub size : usize,
    pub(crate)_marker: PhantomData<F>,
}

impl<F: PrimeField + Ord> Circuit<F> for ExampleCircuit<F> {
    type Config = ExampleConfig<F>;
    type FloorPlanner = V1;
    type Params = ();

    fn without_witnesses(&self) -> Self {
        Self{
            size : self.size,
            _marker: PhantomData,
        }
    }

    fn configure(meta: &mut ConstraintSystem<F>) -> Self::Config {
        Self::Config{gate : ExampleGate::<F>::configure(meta)}
    }

    fn params(&self) -> Self::Params {}

    fn synthesize(&self, mut cfg: Self::Config, mut ly: impl Layouter<F>) -> Result<(), Error> {
        let ly = &mut ly;
        let offset = ly.assign_region(|| "main", |mut region|{
            //let ctx = &mut RegionCtx::new(region);
            let gate = &cfg.gate;
            let s = region.assign_fixed(||"s", gate.s, 0, ||Value::known(F::from(1)))?;
            let a0 = region.assign_advice(|| "a0", gate.a, 0, ||Value::known(F::from(1)))?;
            let a1 = region.assign_advice(|| "a1", gate.a, 1, ||Value::known(F::from(2)))?;
            let b0 = region.assign_advice(|| "b0", gate.b, 0, ||Value::known(F::from(1)))?;
            let c0 = region.assign_advice(|| "c0", gate.c, 0, ||Value::known(F::from(1)))?;
            let a2 = region.assign_advice(||"a2", gate.a, 2, ||Value::known(F::from(2)));
            Ok(())
        });
        Ok(())
    }

}


#[test]
fn run_boundary_test() {
    let mut circuit = ExampleCircuit::<F>{size: 0, _marker: PhantomData};
    let public_inputs = vec![];
    let prover = match MockProver::run(15, &circuit, public_inputs) {
        Ok(prover) => prover,
        Err(e) => panic!("{:#?}", e),
    };
    prover.assert_satisfied();
}

