pub trait PyRepr {
    fn py_repr(&self) -> String;
}

impl PyRepr for bool {
    fn py_repr(&self) -> String {
        if *self { "True" } else { "False" }.to_owned()
    }
}

impl PyRepr for u32 {
    fn py_repr(&self) -> String {
        self.to_string() // integers print the same in both languages
    }
}

impl<T: PyRepr> PyRepr for Option<T> {
    fn py_repr(&self) -> String {
        match self {
            Some(value) => value.py_repr(),
            None => "None".to_owned(),
        }
    }
}

impl PyRepr for f64 {
    fn py_repr(&self) -> String {
        if self.is_nan() {
            return "nan".to_owned();
        }
        if self.is_infinite() {
            return if *self > 0.0 { "inf" } else { "-inf" }.to_owned();
        }
        let magnitude = self.abs();
        if magnitude == 0.0 || (1e-4..1e16).contains(&magnitude) {
            return format!("{self:?}");
        }
        // Python's scientific form: shortest mantissa, signed exits.
        let scientific = format!("{self:e}");
        let (mantissa, exponent) = scientific
            .split_once('e')
            .expect("LowerExp always writes an exponent");
        let exponent: i32 = exponent.parse().expect("LowerExp exponent is an integer");
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}e{sign}{:02}", exponent.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::PyRepr;

    #[test]
    fn bool_is_capitalized() {
        assert_eq!(true.py_repr(), "True");
        assert_eq!(false.py_repr(), "False");
    }

    #[test]
    fn u32_matches_python_int() {
        assert_eq!(0_u32.py_repr(), "0");
        assert_eq!(20_u32.py_repr(), "20");
        assert_eq!(u32::MAX.py_repr(), "4294967295");
    }

    #[test]
    fn option_is_none_or_the_inner_repr() {
        assert_eq!(None::<f64>.py_repr(), "None");
        assert_eq!(Some(5.0_f64).py_repr(), "5.0");
        assert_eq!(Some(true).py_repr(), "True");
        assert_eq!(None::<bool>.py_repr(), "None");
    }

    /// Expected strings are Python's own `repr(float)` output.
    #[test]
    fn f64_matches_python_float() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (5.0, "5.0"),
            (-2.5, "-2.5"),
            (0.1, "0.1"),
            (15.0, "15.0"),
            (1.0 / 3.0, "0.3333333333333333"),
            // Positional/scientific boundary at the small end: Python switches below 1e-4.
            (0.0001, "0.0001"),
            (0.00005, "5e-05"),
            (1e-5, "1e-05"),
            // And at the large end: 1e16 and up is scientific.
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (123456789012345678.0, "1.2345678901234568e+17"),
            // Exponent always signed, at least two digits.
            (1.5e-7, "1.5e-07"),
            (-1e-7, "-1e-07"),
            (1e-300, "1e-300"),
            (5e-324, "5e-324"),
            (f64::MAX, "1.7976931348623157e+308"),
        ];
        for (value, expected) in cases {
            assert_eq!(value.py_repr(), *expected, "repr of {value:?}");
        }
    }

    #[test]
    fn f64_non_finite_matches_python() {
        assert_eq!(f64::NAN.py_repr(), "nan");
        assert_eq!(f64::INFINITY.py_repr(), "inf");
        assert_eq!(f64::NEG_INFINITY.py_repr(), "-inf");
    }
}
