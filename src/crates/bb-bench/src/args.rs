//! The arguments after the verb, consumed left to right, and the three ways a command can fail.

/// What stops a command: a command line that makes no sense (exit 64), or a broken run or
/// environment (exit 2).
#[derive(Debug)]
pub enum Fail {
    Usage(String),
    Error(String),
}

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Self {
        Fail::Error(e.to_string())
    }
}

impl From<bb_core::benchmarking::BenchError> for Fail {
    fn from(e: bb_core::benchmarking::BenchError) -> Self {
        Fail::Error(e.to_string())
    }
}

impl From<String> for Fail {
    fn from(e: String) -> Self {
        Fail::Error(e)
    }
}

pub struct ArgList {
    args: Vec<String>,
    at: usize,
}

impl ArgList {
    pub fn new(args: Vec<String>) -> ArgList {
        ArgList { args, at: 0 }
    }

    pub fn next_arg(&mut self) -> Option<String> {
        let arg = self.args.get(self.at).cloned();
        if arg.is_some() {
            self.at += 1;
        }
        arg
    }

    /// The value after a flag, or a usage error naming the flag.
    pub fn value(&mut self, flag: &str) -> Result<String, Fail> {
        self.next_arg()
            .ok_or_else(|| Fail::Usage(format!("{flag} needs a value.")))
    }

    pub fn int(&mut self, flag: &str) -> Result<u32, Fail> {
        let text = self.value(flag)?;
        text.parse::<u32>().map_err(|_| {
            Fail::Usage(format!(
                "{flag} wants a non-negative whole number, got '{text}'."
            ))
        })
    }

    pub fn double(&mut self, flag: &str) -> Result<f64, Fail> {
        let text = self.value(flag)?;
        match text.parse::<f64>() {
            Ok(v) if v >= 0.0 && v.is_finite() => Ok(v),
            _ => Err(Fail::Usage(format!(
                "{flag} wants a non-negative number, got '{text}'."
            ))),
        }
    }
}

pub fn unknown(verb: &str, arg: &str) -> Fail {
    Fail::Usage(format!("{verb}: unrecognised argument '{arg}'."))
}
