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
