use ff::{PrimeField, Field};
use group::{Curve, prime::PrimeCurveAffine};
use num_bigint::{BigUint, RandomBits};
use num_traits::Num;
use halo2::{arithmetic::{self, parallelize, best_fft, FftGroup, eval_polynomial, kate_division, best_multiexp}, poly::{commitment::MSM, ipa::{msm::MSMIPA, commitment::ParamsIPA}}};
use halo2curves::{CurveAffine, bn256, grumpkin, Coordinates, CurveExt};
use halo2curves::bn256::Fr as F;
use halo2curves::grumpkin::Fr as Fq;
use rand_core::OsRng;
use subtle::CtOption;
use std::{ops::{Shl, Add, Mul, Shr}, cmp, iter::*, fmt::{Display, Formatter}, time::SystemTime};
use rand::{Rng, random};

type Grumpkin = grumpkin::G1Affine;

#[derive(Clone)]
pub struct Polynomial <F: PrimeField> { // this seems to be re-doing some work from halo2::poly...
    pub poly: Vec<F>,
}

pub fn poly<T:IntoIterator>(it: T) -> Polynomial<T::Item> where T::Item : PrimeField {
    Polynomial::new(it.into_iter().collect())
}

impl<F: PrimeField> Polynomial<F>{

    pub fn new(poly: Vec<F>)->Self{
        Polynomial{poly}
    }

    pub fn ev(&self, x: F)-> F{
        eval_polynomial(&self.poly, x)
    }

    pub fn kate_div(&self, b: F) -> Self{
        Polynomial::new(kate_division(&self.poly, b))
    }

    pub fn scale(&self, sc: F) -> Self{
        Polynomial::new((&self.poly).into_iter().map(|x|*x*sc).collect())
    }


    pub fn mul_naive(a: &Self, b: &Self) -> Self{
        let mut ret : Vec<F> = repeat(F::ZERO).take(a.poly.len()+b.poly.len()-1).collect();
        for i in 0..a.poly.len() {
            for j in 0..b.poly.len() {
                ret[i+j]+=a.poly[i]*b.poly[j];                
            }
        }
        Polynomial::new(ret)
    }

    /// according to my tests, this functions is absolutely useless;
    /// either naive or fft multiplication is always better
    pub fn mul_karatsuba(a: &Self, b: &Self) -> Self{
        let d = cmp::max(a.poly.len(), b.poly.len())/2;

        if a.poly.len()<=1 || b.poly.len()<=1 {
            return Self::mul_naive(a, b)
        }

        let mut a_l = vec![];
        let mut a_r = vec![];
        let mut b_l = vec![];
        let mut b_r = vec![];
        for i in 0..a.poly.len() {
            if i<d {a_l.push(a.poly[i])} else {a_r.push(a.poly[i])}
        }
        for i in 0..b.poly.len() {
            if i<d {b_l.push(b.poly[i])} else {b_r.push(b.poly[i])}
        }

        let a_l = Polynomial::new(a_l);
        let a_r = Polynomial::new(a_r);
        let b_l = Polynomial::new(b_l);
        let b_r = Polynomial::new(b_r);

        let a_s = &a_l + &a_r;
        let b_s = &b_l + &b_r;

        let m0 = Self::mul_karatsuba(&a_l, &b_l);
        let m2 = Self::mul_karatsuba(&a_r, &b_r);
        let m1 = &Self::mul_karatsuba(&a_s, &b_s) + &(&m0 + &m2).scale(-F::ONE);

        // answer is m0 + m1 x^d + m2 x^{2d}

        &(&m0 + &(&m1>>d)) + &(&m2>>(2*d))
    }


    pub fn mul_fft(&self, other: &Self) -> Self{

        let length = self.poly.len() + other.poly.len()-1;
        let loglength = log2_floor(length)+1;

        let fgsds = F::ZERO; // amulet of protection against crab demons

        let padded_length = (2 as usize).pow(loglength);
        let mut a : Vec<F> = (&self.poly).into_iter().chain(repeat(&fgsds)).take(padded_length).map(|x|*x).collect();
        let mut b : Vec<F> = (&other.poly).into_iter().chain(repeat(&fgsds)).take(padded_length).map(|x|*x).collect();
        assert!(F::S >= loglength);
        let omega = F::ROOT_OF_UNITY.pow([(2 as u64).pow((F::S-loglength))]); //this will produce a root of unity of order loglength
        let omega_inv = F::ROOT_OF_UNITY_INV.pow([(2 as u64).pow((F::S-loglength))]);


        let scaling = F::from_str_vartime(&format!("{}", padded_length)).unwrap().invert().unwrap();

        best_fft(&mut a, omega, loglength);
        best_fft(&mut b, omega, loglength);

        let mut prod : Vec<F> = a.into_iter().zip(b.into_iter()).map(|(x,y)|x*y*scaling).collect();
        
        best_fft(&mut prod, omega_inv, loglength);


        Polynomial::new(prod.into_iter().take(length).collect())
        
    }

}


impl<F: PrimeField> Display for Polynomial<F>{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error>{
        let poly = &self.poly;
        write!(
            f,
            "{}",
            poly
                .into_iter()
                .enumerate()
                .fold("".to_string(),
                     |acc, (id, val)|
                            format!(
                                "{}{}*x^{}", 
                                if id>0 
                                    {format!("{} +", acc)} 
                                else 
                                    {"".to_string()},
                                val
                                    .to_repr()
                                    .as_ref()
                                    .into_iter()
                                    .fold(
                                        "".to_string(),
                                        |acc, val| 
                                            format!("{:02x}{}", val, acc)
                                        ), 
                                    id
                                )
                        )
            )   
    }
}

impl<F: PrimeField> Shr<usize> for &Polynomial<F>{

    type Output = Polynomial<F>;

    fn shr(self, other:usize) -> Self::Output{
        let tmp : Vec<F> = repeat(F::ZERO).take(other).chain(self.poly.iter().map(|x|*x)).collect();
        Polynomial::new(tmp)
    }

}

impl<F: PrimeField> Add for &Polynomial<F>{
    
    type Output = Polynomial<F>;

    fn add(self, other: Self) -> Self::Output{
        let max_len = cmp::max(self.poly.len(), other.poly.len());

        let fgsds = F::ZERO; // amulet of protection against crab demons

        let sum : Vec<F> =
            (&self.poly).into_iter().chain(repeat(&fgsds))
            .zip((&other.poly).into_iter().chain(repeat(&fgsds)))
            .map(|(x,y)|*x+y)
            .take(max_len).collect();

        Polynomial::new(sum)
    }
}

fn log2_floor(num: usize) -> u32 {
    assert!(num > 0);

    let mut pow = 0;

    while (1 << (pow + 1)) <= num {
        pow += 1;
    }

    pow
}

impl<F:PrimeField> Mul for &Polynomial<F>{
    type Output = Polynomial<F>;

    fn mul(self, other: Self) -> Self::Output{
        if (*self).poly.len() < 32 || other.poly.len() < 32 { return Polynomial::mul_naive(self, other) }
        self.mul_fft(other)
    }
}



#[derive(Clone)]
/// A function of the form a(x) + y*b(x) on a curve.
pub struct RegularFunction<C: CurveAffine>{
    a: Polynomial<C::Base>,
    b: Polynomial<C::Base>,
}

impl<C: CurveAffine> RegularFunction<C>{
    pub fn ev(&self, pt: C) -> C::Base{
        let pt = pt.coordinates().unwrap();
        self.a.ev(*pt.x()) + self.b.ev(*pt.x())*pt.y()
    }

    pub fn ev_unchecked(self, x: C::Base, y: C::Base) -> C::Base{
        self.a.ev(x) + self.b.ev(x)*y
    }

    pub fn from_const(x: C::Base) -> Self{
        Self::new(Polynomial::new(vec![x]), Polynomial::new(vec![]))
    }

    pub fn new(a: Polynomial<C::Base>, b: Polynomial<C::Base>) -> Self{
        RegularFunction { a, b }
    }

    pub fn scale(&self, sc: C::Base) -> Self{
        RegularFunction { a: self.a.scale(sc), b: self.b.scale(sc) }        
    }
}

impl<C: CurveAffine> Add for &RegularFunction<C>{
    type Output = RegularFunction<C>;

    fn add(self, other: Self) -> Self::Output{
        RegularFunction::new(&self.a+&other.a, &self.b+&other.b)
    }
}


impl<C: CurveAffine> Mul for &RegularFunction<C>{
    type Output = RegularFunction<C>;

    fn mul(self, other: Self) -> Self::Output{
        let subst_y2 = Polynomial::new(vec![C::b(), C::a(), C::Base::ZERO, C::Base::ONE]); // x^3 + ax + b
        RegularFunction::new(&(&self.a*&other.a) + &(&(&self.b*&other.b)*&subst_y2), &(&self.a*&other.b) + &(&self.b*&other.a))
    }
}

/// Idiotic way of converting value to a montgomery arithmetic. from_repr is angry at me and I don't understand why.
fn felt_from_u64<Fz: PrimeField>(d: u64) -> Fz {
    Fz::from(d) * Fz::from(1 as u64).invert().unwrap()
}

// polynomials code probably should be replaced by some proper code at some point
// importantly, always using FFT is inefficient, for some ranges Karatsuba is better
// now, the interesting part starts

/// this function returns a line passing through a pair of points
/// highest coefficient is guaranteed to be 1, i.e. it is always of the form y+ax+b or x+a
/// if both points are 0, it will panic
pub fn linefunc<C: CurveAffine>(a: &C, b: &C) -> RegularFunction<C>{
    let _a:Option<Coordinates<C>> = a.coordinates().into();
    let _b:Option<Coordinates<C>> = b.coordinates().into();

    // process cases where a or b are 0
    match (_a,_b) {
        (None, None) => panic!(),
        (Some(q), None) => return
                            RegularFunction::new(
                                Polynomial::new(vec![-*q.x(), C::Base::ONE]),
                                Polynomial::new(vec![C::Base::ZERO])
                                )
                            ,
        (Some(q), None) => return
                            RegularFunction::new(
                                Polynomial::new(vec![-*q.x(), C::Base::ONE]),
                                Polynomial::new(vec![C::Base::ZERO])
                                )
                            ,   
        _ => (),
    }

    // another case with vertical line, case where a = -b
    // note: this unwrap never fails, because a and c = 0 implies b = 0, which would panic on the previous step
    if *a == -*b {return RegularFunction::new(
                            Polynomial::new(vec![-*_a.unwrap().x(), C::Base::ONE]),
                            Polynomial::new(vec![C::Base::ZERO])
        )}

    let c = -(*a+*b);
    let c : &C = &c.into();

    let b = if a == b {c} else {b};// if a == b we replace b by c, to avoid dealing with tangent case

    let a_coords = a.coordinates().unwrap();
    let ax = a_coords.x();
    let ay = a_coords.y();

    let b_coords = b.coordinates().unwrap();
    let bx = b_coords.x();
    let by = b_coords.y();

    // create function (bx-ax)y - (by-ay)x
    let mut line = RegularFunction::<C>::new(
                    Polynomial::new(vec![C::Base::ZERO, *ay - *by]),
                    Polynomial::new(vec![*bx - *ax])
                );
    
    // offset to ensure it vanishes in point a
    line = &line + &RegularFunction::from_const(-line.ev(*a));

    // rescale by coefficient of y
    line.scale(line.b.poly[0].invert().unwrap())
}

#[derive(Clone)]
/// this struct holds the following data:
/// 1) vector of "input" points
/// 2) additional "output" point, such that sum of inputs + output = 0
/// 3) a regular function which vanishes exactly in all inputs and output
/// they can be merged together by composing outputs
pub struct Propagation<C: CurveAffine>{
    inputs: Vec<C>,
    output: C,
    wtns: RegularFunction<C>
}

impl<C: CurveAffine> Propagation<C>{
    
    pub fn from_point(pt: C) -> Self{
        if pt == C::identity() {return Self::empty()}
        Propagation{inputs: vec![pt], output: -pt, wtns: linefunc(&pt, &(-pt))}
    }
    
    pub fn empty() -> Self{
        Propagation{inputs: vec![], output: C::identity(), wtns: RegularFunction { a: Polynomial::new(vec![C::Base::ONE]), b: Polynomial::new(vec![]) }}
    }

    pub fn merge(a: Self, b: Self) -> Self {
        let inputs = a.inputs.into_iter().chain(b.inputs.into_iter()).collect();
        let output = a.output+b.output;
        
        let ao = a.output.coordinates();
        let bo = b.output.coordinates();

        let ao : Option<Coordinates<C>> = ao.into();
        let bo : Option<Coordinates<C>> = bo.into();
        
        match (ao, bo) {

            (None, _) => {
                let wtns = &a.wtns*&b.wtns;
                return Propagation{ inputs, output: output.into(), wtns }
                },
            (_, None) => {
                let wtns = &a.wtns*&b.wtns;
                return Propagation{ inputs, output: output.into(), wtns }
                },
            _=>()
        };

        let ao = ao.unwrap();
        let bo = bo.unwrap();

        let ax = ao.x();
        let bx = bo.x();

        let numerator = &a.wtns * &(&b.wtns * &linefunc(&(-a.output), &(-b.output)));

        let num_a = numerator.a;
        let num_b = numerator.b;

        let wtns = RegularFunction::new(num_a.kate_div(*ax).kate_div(*bx), num_b.kate_div(*ax).kate_div(*bx));

        Propagation { inputs, output: output.into(), wtns }
    }


    pub fn maybe_merge(m: MaybePair<C>) -> Self{
        match m {
            MaybePair::Unit(x) => x,
            MaybePair::Pair(x,y) => Self::merge(x,y)
        }
    }

    pub fn update_mpair_vec(pairs: &mut Vec<MaybePair<C>>, upd: Self) -> () {
        let l = pairs.len();
        if l == 0 {pairs.push(MaybePair::Unit(upd))} else {
            match &pairs[l-1] {
                MaybePair::Pair(..) => pairs.push(MaybePair::Unit(upd)),
                MaybePair::Unit(x) => pairs[l-1] = MaybePair::Pair(x.clone(), upd)
            }
        }
    }

    pub fn group_merge(arr: Vec<Self>) -> Self{
        if arr.len() == 0 {panic!()};
        if arr.len() == 1 {return arr[0].clone()}
        let mut pairs  = vec![];
        for q in arr.into_iter(){
            Self::update_mpair_vec(&mut pairs, q);
        }
        let mut tmp : Vec<MaybePairGlue<C>> = pairs.into_iter().map(|x|MaybePairGlue::In(x)).collect();
        parallelize(&mut tmp, |chunk, _|
            for x in chunk.iter_mut() {
                let store;
                let tmp = x.clone();
                match tmp {
                    MaybePairGlue::In(m) => {let tmp = Self::maybe_merge(m); store = MaybePairGlue::Out(tmp)},
                    _ => panic!(),
                    };
                *x = store;
                }
            );

        Self::group_merge(tmp.into_iter().map(|x| match x {MaybePairGlue::Out(p) => p, _ => panic!()}).collect())
    }


}


#[derive(Clone)]
pub enum MaybePair<C: CurveAffine>{
    Unit(Propagation<C>),
    Pair(Propagation<C>, Propagation<C>),
}

#[derive(Clone)]
/// this atrocity is needed to call parallelize
pub enum MaybePairGlue<C: CurveAffine>{
    In(MaybePair<C>),
    Out(Propagation<C>),
}

// utility functions for testing

pub fn gen_random_pt<C: CurveAffine>() -> C {
    let tmp : u128 = random();
    let hasher = C::CurveExt::hash_to_curve("TEST ONLY");
    hasher(&tmp.to_le_bytes()).to_affine()
}


/// computes a regular function vanishing in a collection of points and minus their sum
pub fn compute_divisor_witness_partial<C: CurveAffine>(pts: Vec<C>)-> (RegularFunction<C>, C) {
    let tmp = Propagation::group_merge(pts.into_iter().map(Propagation::from_point).collect());
    (tmp.wtns, tmp.output)
}

/// computes a regular function vanishing in a collection of points, panics if the sum is nonzero
pub fn compute_divisor_witness<C: CurveAffine>(pts: Vec<C>)-> RegularFunction<C> {
    let tmp = compute_divisor_witness_partial(pts);
    if tmp.1 != C::identity() {panic!()}
    tmp.0
}


 #[test]

 fn poly_test(){
    
    let p = Polynomial::new(repeat(F::random(OsRng)).take(100).collect());
    let q = Polynomial::new(repeat(F::random(OsRng)).take(423).collect());

    let reg : RegularFunction<Grumpkin> = RegularFunction::new(p.clone(), q.clone());

    let t = F::random(OsRng);

    assert_eq!(p.ev(t)+q.ev(t), (&p+&q).ev(t)); // test addition in random point
    assert_eq!(p.ev(t)*q.ev(t), (&p*&q).ev(t)); // test multiplication in random point

    let p2 = &p + &poly([-p.ev(t)]);
    let q = p.kate_div(t);

    let t2 = F::random(OsRng);
    assert_eq!(p2.ev(t2), q.ev(t2)*(t2-t)); // test division (+ the fact that it ignores remainder)

    let r = Polynomial::new(repeat(F::ZERO).take(5).collect());
    let s = r.kate_div(t);
    
    assert_eq!(s.poly.len(),4); // checking that division does not pad leading zeros

}

#[test]

fn karatsuba_test(){
    let p = Polynomial::new(repeat(F::random(OsRng)).take(100).collect());
    let q = Polynomial::new(repeat(F::random(OsRng)).take(423).collect());
    let t = F::random(OsRng);
    assert_eq!(p.ev(t)*q.ev(t), Polynomial::mul_karatsuba(&p, &q).ev(t)); // test multiplication in random point

}

#[test]

fn bench_naive(){

    for i in 1..100 {
        let p = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
        let q = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
    
        let start = SystemTime::now();
        for _ in 0..10 {
            let _ = Polynomial::mul_naive(&p, &q);
        }
        println!("Time elapsed: {} ms; deg={}", start.elapsed().unwrap().as_millis(), i);
    }
}

#[test]

fn bench_karatsuba(){
    for i in 1..100 {
        let p = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
        let q = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
    
        let start = SystemTime::now();
        for _ in 0..10 {
            let _ = Polynomial::mul_karatsuba(&p, &q);
        }
        println!("Time elapsed: {} ms; deg={}", start.elapsed().unwrap().as_millis(), i);
    }}

#[test]

fn bench_best(){
    for i in 1..100 {
        let p = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
        let q = Polynomial::new(repeat(F::random(OsRng)).take(i).collect());
    
        let start = SystemTime::now();
        for _ in 0..10 {
            let _ = &p*&q;
        }
        println!("Time elapsed: {} ms; deg={}", start.elapsed().unwrap().as_millis(), i);
    }
}


#[test]

fn randpoints_witness_test(){
    let mut scalars : Vec<Fq> = repeat(Fq::ONE).take(500).collect();
    let mut pts : Vec<Grumpkin> = repeat(gen_random_pt()).take(500).collect();
    let res = best_multiexp(&scalars, &pts);
    pts.push((-res).into());
    scalars.push(Fq::ONE);

    let regf = compute_divisor_witness(pts.clone());

    let _ : Vec<()> = pts.into_iter().map(|pt| assert!(regf.ev(pt) == F::ZERO)).collect();
}