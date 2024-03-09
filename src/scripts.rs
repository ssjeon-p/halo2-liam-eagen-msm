use halo2_backend::arithmetic::Field;
use halo2curves::bn256::Fr as F;
use halo2curves::ff::PrimeField;
use halo2curves::serde::SerdeObject;
use std::{fs::File, io::Write};

#[test]

// this could have been a procedural macro, I guess, but I'm bad
fn precompute_fft_aux_data() {
    let mut s: String = "".to_string();
    s += "use halo2curves::{bn256::Fr as F, serde::SerdeObject};\n";
    s += "use crate::regular_functions_utils::FftPrecomp;\n";
    s += "impl FftPrecomp for F {\n";
    s += "    fn omega_pow(exp2: u32) -> F {\n";
    s += "        let tmp = match exp2 {\n";
    for i in 0..64 {
        s += &format!(
            "            {i}=>{:?},\n",
            F::ROOT_OF_UNITY.pow([2_u64.pow(i as u32)]).to_raw_bytes()
        );
    }
    s += "            _=>panic!(),\n";
    s += "        };\n";
    s += "    F::from_raw_bytes_unchecked(&tmp)\n";
    s += "    }\n\n";

    s += "    fn omega_pow_inv(exp2: u32) -> F {\n";
    s += "        let tmp = match exp2 {\n";
    for i in 0..64 {
        s += &format!(
            "            {i}=>{:?},\n",
            F::ROOT_OF_UNITY_INV
                .pow([2_u64.pow(i as u32)])
                .to_raw_bytes()
        );
    }
    s += "            _=>panic!(),\n";
    s += "        };\n";
    s += "    F::from_raw_bytes_unchecked(&tmp)\n";
    s += "    }\n\n";

    s += "    fn half_pow(exp: u64) -> F {\n";
    s += "        let tmp = match exp {\n";
    for i in 0..64 {
        s += &format!(
            "            {i}=>{:?},\n",
            F::TWO_INV.pow([i as u64]).to_raw_bytes()
        );
    }
    s += "            _=>panic!(),\n";
    s += "        };\n";
    s += "    F::from_raw_bytes_unchecked(&tmp)\n";
    s += "    }\n";
    s += "}\n";

    let mut f = File::create("./src/precomputed_fft_data.rs").expect("Unable to create file");
    f.write_all(s.as_bytes()).expect("Unable to write data");
}
