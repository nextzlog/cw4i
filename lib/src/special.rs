#![allow(clippy::excessive_precision)]
//! Special functions missing from the standard library: the complementary error
//! function and the IEEE 754 remainder.

/// Complementary error function (rational approximations of the FreeBSD msun library).
pub fn erfc(x: f64) -> f64 {
    const ERX: f64 = 8.450_629_115_104_675_292_97e-01;
    const PP0: f64 = 1.283_791_670_955_125_585_61e-01;
    const PP1: f64 = -3.250_421_072_470_014_993_70e-01;
    const PP2: f64 = -2.848_174_957_559_851_047_66e-02;
    const PP3: f64 = -5.770_270_296_489_441_591_57e-03;
    const PP4: f64 = -2.376_301_665_665_016_260_84e-05;
    const QQ1: f64 = 3.979_172_239_591_553_528_19e-01;
    const QQ2: f64 = 6.502_224_998_876_729_444_85e-02;
    const QQ3: f64 = 5.081_306_281_875_765_627_76e-03;
    const QQ4: f64 = 1.324_947_380_043_216_445_26e-04;
    const QQ5: f64 = -3.960_228_278_775_368_123_20e-06;
    const PA0: f64 = -2.362_118_560_752_659_440_77e-03;
    const PA1: f64 = 4.148_561_186_837_483_316_66e-01;
    const PA2: f64 = -3.722_078_760_357_013_238_47e-01;
    const PA3: f64 = 3.183_466_199_011_617_536_74e-01;
    const PA4: f64 = -1.108_946_942_823_966_774_76e-01;
    const PA5: f64 = 3.547_830_432_561_823_593_71e-02;
    const PA6: f64 = -2.166_375_594_868_790_843_00e-03;
    const QA1: f64 = 1.064_208_804_008_442_282_86e-01;
    const QA2: f64 = 5.403_979_177_021_710_489_37e-01;
    const QA3: f64 = 7.182_865_441_419_626_628_68e-02;
    const QA4: f64 = 1.261_712_198_087_616_421_12e-01;
    const QA5: f64 = 1.363_708_391_202_905_073_62e-02;
    const QA6: f64 = 1.198_449_984_679_910_741_70e-02;
    const RA0: f64 = -9.864_944_034_847_148_227_05e-03;
    const RA1: f64 = -6.938_585_727_071_817_643_72e-01;
    const RA2: f64 = -1.055_862_622_532_329_098_14e+01;
    const RA3: f64 = -6.237_533_245_032_600_603_96e+01;
    const RA4: f64 = -1.623_966_694_625_734_703_55e+02;
    const RA5: f64 = -1.846_050_929_067_110_359_94e+02;
    const RA6: f64 = -8.128_743_550_630_659_342_46e+01;
    const RA7: f64 = -9.814_329_344_169_145_485_92e+00;
    const SA1: f64 = 1.965_127_166_743_925_712_92e+01;
    const SA2: f64 = 1.376_577_541_435_190_426_00e+02;
    const SA3: f64 = 4.345_658_774_752_292_288_21e+02;
    const SA4: f64 = 6.453_872_717_332_678_803_36e+02;
    const SA5: f64 = 4.290_081_400_275_678_333_86e+02;
    const SA6: f64 = 1.086_350_055_417_794_351_34e+02;
    const SA7: f64 = 6.570_249_770_319_281_701_35e+00;
    const SA8: f64 = -6.042_441_521_485_809_874_38e-02;
    const RB0: f64 = -9.864_942_924_700_099_285_97e-03;
    const RB1: f64 = -7.992_832_376_805_230_065_74e-01;
    const RB2: f64 = -1.775_795_491_775_475_198_89e+01;
    const RB3: f64 = -1.606_363_848_558_219_160_62e+02;
    const RB4: f64 = -6.375_664_433_683_896_277_22e+02;
    const RB5: f64 = -1.025_095_131_611_077_249_54e+03;
    const RB6: f64 = -4.835_191_916_086_513_970_19e+02;
    const SB1: f64 = 3.033_806_074_348_245_829_24e+01;
    const SB2: f64 = 3.257_925_129_965_739_188_26e+02;
    const SB3: f64 = 1.536_729_586_084_436_959_94e+03;
    const SB4: f64 = 3.199_858_219_508_595_539_08e+03;
    const SB5: f64 = 2.553_050_406_433_164_425_83e+03;
    const SB6: f64 = 4.745_285_412_069_553_672_15e+02;
    const SB7: f64 = -2.244_095_244_658_581_833_62e+01;
    const TINY: f64 = 1.0 / (1u64 << 56) as f64;
    if x.is_nan() {
        return f64::NAN;
    }
    if x == f64::INFINITY {
        return 0.0;
    }
    if x == f64::NEG_INFINITY {
        return 2.0;
    }
    let mut x = x;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if x < 0.84375 {
        let temp = if x < TINY {
            x
        } else {
            let z = x * x;
            let r = PP0 + z * (PP1 + z * (PP2 + z * (PP3 + z * PP4)));
            let s = 1.0 + z * (QQ1 + z * (QQ2 + z * (QQ3 + z * (QQ4 + z * QQ5))));
            let y = r / s;
            if x < 0.25 {
                x + x * y
            } else {
                0.5 + (x * y + (x - 0.5))
            }
        };
        return if sign { 1.0 + temp } else { 1.0 - temp };
    }
    if x < 1.25 {
        let s = x - 1.0;
        let p = PA0 + s * (PA1 + s * (PA2 + s * (PA3 + s * (PA4 + s * (PA5 + s * PA6)))));
        let q = 1.0 + s * (QA1 + s * (QA2 + s * (QA3 + s * (QA4 + s * (QA5 + s * QA6)))));
        return if sign {
            1.0 + ERX + p / q
        } else {
            1.0 - ERX - p / q
        };
    }
    if x < 28.0 {
        let s = 1.0 / (x * x);
        let (r, ss) = if x < 1.0 / 0.35 {
            (
                RA0 + s
                    * (RA1 + s * (RA2 + s * (RA3 + s * (RA4 + s * (RA5 + s * (RA6 + s * RA7)))))),
                1.0 + s
                    * (SA1
                        + s * (SA2
                            + s * (SA3 + s * (SA4 + s * (SA5 + s * (SA6 + s * (SA7 + s * SA8))))))),
            )
        } else {
            if sign && x > 6.0 {
                return 2.0;
            }
            (
                RB0 + s * (RB1 + s * (RB2 + s * (RB3 + s * (RB4 + s * (RB5 + s * RB6))))),
                1.0 + s
                    * (SB1 + s * (SB2 + s * (SB3 + s * (SB4 + s * (SB5 + s * (SB6 + s * SB7)))))),
            )
        };
        let z = f64::from_bits(x.to_bits() & 0xffff_ffff_0000_0000);
        let rr = (-z * z - 0.5625).exp() * ((z - x) * (z + x) + r / ss).exp();
        return if sign { 2.0 - rr / x } else { rr / x };
    }
    if sign {
        2.0
    } else {
        0.0
    }
}

/// IEEE 754 remainder of x / y: x - n y with n the integer nearest to x / y (ties to even).
pub fn remainder(x: f64, y: f64) -> f64 {
    const TINY: f64 = 4.450_147_717_014_402_766_18e-308;
    const HALF_MAX: f64 = f64::MAX / 2.0;
    if x.is_nan() || y.is_nan() || x.is_infinite() || y == 0.0 {
        return f64::NAN;
    }
    if y.is_infinite() {
        return x;
    }
    let mut x = x;
    let mut y = y;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if y < 0.0 {
        y = -y;
    }
    if x == y {
        return if sign { -0.0 } else { 0.0 };
    }
    if y <= HALF_MAX {
        x %= y + y;
    }
    if y < TINY {
        if x + x > y {
            x -= y;
            if x + x >= y {
                x -= y;
            }
        }
    } else {
        let y_half = 0.5 * y;
        if x > y_half {
            x -= y;
            if x >= y_half {
                x -= y;
            }
        }
    }
    if sign {
        -x
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erfc_values() {
        // tabulated values of erfc
        let cases = [
            (0.0, 1.0),
            (0.1, 0.887_537_083_981_715_1),
            (0.5, 0.479_500_122_186_953_5),
            (1.0, 0.157_299_207_050_285_13),
            (2.0, 0.004_677_734_981_047_266),
            (3.0, 2.209_049_699_858_544e-5),
            (5.0, 1.537_459_794_428_034_8e-12),
            (-1.0, 1.842_700_792_949_715),
        ];
        for (x, want) in cases {
            let got = erfc(x);
            assert!(
                (got - want).abs() <= 1e-15 * want.abs().max(1e-300) * 4.0,
                "erfc({x}) = {got}, want {want}"
            );
        }
        assert_eq!(erfc(30.0), 0.0);
        assert_eq!(erfc(-30.0), 2.0);
        assert_eq!(erfc(f64::INFINITY), 0.0);
        assert!(erfc(f64::NAN).is_nan());
    }

    #[test]
    fn remainder_values() {
        let two_pi = 2.0 * std::f64::consts::PI;
        assert!((remainder(7.0, two_pi) - (7.0 - two_pi)).abs() < 1e-15);
        assert!((remainder(-7.0, two_pi) + (7.0 - two_pi)).abs() < 1e-15);
        assert_eq!(remainder(5.0, 2.0), 1.0);
        assert_eq!(remainder(7.0, 2.0), -1.0); // ties to even
        assert_eq!(remainder(3.0, 2.0), -1.0);
        assert!(remainder(1.0, 0.0).is_nan());
        assert_eq!(remainder(1.5, f64::INFINITY), 1.5);
    }
}
