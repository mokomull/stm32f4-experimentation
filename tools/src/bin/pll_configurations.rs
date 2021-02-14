use std::ops::RangeInclusive;

use itertools::Itertools;

type Rational = num_rational::Rational64;

// VCO input must be between 1MHz and 2MHz, per documentation for the PLLM bits of RCC_PLLCFGR
static VALID_VCO_INPUT: RangeInclusive<Rational> =
    Rational::new_raw(1_000_000, 1)..=Rational::new_raw(2_000_000, 1);
// VCO output must be between 100MHz and 432MHz, per documentation for the PLLN bits of RCC_PLLCFGR
static VALID_VCO_OUTPUT: RangeInclusive<Rational> =
    Rational::new_raw(100_000_000, 1)..=Rational::new_raw(432_000_000, 1);
// PLL_OUT must be between 24MHz and 168MHz, per "PLL Characteristics" in the STM32F407 datasheet
static VALID_PLL_OUT: RangeInclusive<Rational> =
    Rational::new_raw(24_000_000, 1)..=Rational::new_raw(168_000_000, 1);
// PLL48_OUT must be between 48MHz and 75MHz, per the same table
static VALID_PLL48_OUT: RangeInclusive<Rational> =
    Rational::new_raw(48_000_000, 1)..=Rational::new_raw(75_000_000, 1);

// For now, print the I2S settings that get us within 1% of 12.288MHz
static DESIRED_I2S_MCLK: RangeInclusive<Rational> =
    Rational::new_raw(12_165_120, 1)..=Rational::new_raw(12_410_880, 1);

fn main() {
    let m_range = 2..=63;
    let n_range = 50..=432;
    let p_range = vec![2, 4, 6, 8].into_iter();
    let q_range = 2..=15;
    let i2sn_range = 50..=432;
    let i2sr_range = 2..=7;
    let i2sdiv_range = 2..=255;
    let odd_range = 0..=1;

    let hse = Rational::new(11_059_200.into(), 1.into());

    let mut count: u64 = 0;

    for m in m_range {
        let vco_input = hse / m;
        if !VALID_VCO_INPUT.contains(&vco_input) {
            continue;
        }

        for n in n_range.clone() {
            let vco_output = vco_input * n;
            if !VALID_VCO_OUTPUT.contains(&vco_output) {
                continue;
            }

            for p in p_range.clone() {
                let pll_clk = vco_output / p;
                if !VALID_PLL_OUT.contains(&pll_clk) {
                    continue;
                }

                for q in q_range.clone().rev() {
                    let pll48_clk = vco_output / q;
                    if !VALID_PLL48_OUT.contains(&pll48_clk) {
                        continue;
                    }

                    for i2sn in i2sn_range.clone() {
                        let i2s_vco_output = vco_input * i2sn;
                        if !VALID_VCO_OUTPUT.contains(&i2s_vco_output) {
                            continue;
                        }

                        for i2sr in i2sr_range.clone() {
                            let i2s_clock = i2s_vco_output / i2sr;
                            if !VALID_PLL_OUT.contains(&i2s_clock) {
                                continue;
                            }

                            for (i2sdiv, odd) in
                                i2sdiv_range.clone().cartesian_product(odd_range.clone())
                            {
                                let mclk = i2s_clock / (2 * i2sdiv + odd);
                                if !DESIRED_I2S_MCLK.contains(&mclk) {
                                    continue;
                                }

                                count += 1;
                            }
                        }
                    }

                    // only check the largest value of `q` that fits the criteria, corresponding to
                    // the slowest PLL48 clock we can make with the chosen M and N settings -- i.e.
                    // the closest to 48MHz.
                    break;
                }

                // only check the smallest value of `p` that fits the criteria, corresponding to the
                // fastest core clock we can make with the already-chosen M and N settings.
                break;
            }
        }
    }

    dbg!(count);
}
