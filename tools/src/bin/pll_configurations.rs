use std::ops::RangeInclusive;

use itertools::iproduct;

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

fn main() {
    let m_range = 2..=63;
    let n_range = 50..=432;
    let p_range = vec![2, 4, 6, 8].into_iter();
    let q_range = 2..=15;
    let i2sn_range = 50..=432;
    let i2sr_range = 2..=7;

    let hse = Rational::new(11_059_200.into(), 1.into());

    let vco_inputs = m_range.filter_map(|m| {
        let vco_input = hse / m;
        VALID_VCO_INPUT.contains(&vco_input).then(|| vco_input)
    });

    let mut count: u64 = 0;

    for (vco_input, n, p, q, i2sn, i2sr) in
        iproduct!(vco_inputs, n_range, p_range, q_range, i2sn_range, i2sr_range)
    {
        let vco_output = vco_input * n;
        if !VALID_VCO_OUTPUT.contains(&vco_output) {
            continue;
        }

        let pll_clk = vco_output / p;
        if !VALID_PLL_OUT.contains(&pll_clk) {
            continue;
        }

        let pll48_clk = vco_output / q;
        if !VALID_PLL48_OUT.contains(&pll48_clk) {
            continue;
        }

        let i2s_vco_output = vco_input * i2sn;
        if !VALID_VCO_OUTPUT.contains(&i2s_vco_output) {
            continue;
        }

        let i2s_clock = i2s_vco_output / i2sr;
        if !VALID_PLL_OUT.contains(&i2s_clock) {
            continue;
        }

        count += 1;
    }

    dbg!(count);
}
