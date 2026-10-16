use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf};
pub struct Args {
    pub words: Vec<OsString>,
    pub flags: BTreeMap<String, OsString>,
}
impl Args {
    pub fn validate(&self) -> io::Result<()> {
        if self.flags.contains_key("help") {
            return Ok(());
        }
        let (min, max, allowed): (usize, usize, &[&str]) = match self.word(0)?.as_str() {
            "scan" => (
                2,
                2,
                &[
                    "case",
                    "workers",
                    "journal",
                    "resume",
                    "scan",
                    "progress",
                    "pause-after",
                ],
            ),
            "inspect" | "verify" => (2, 2, &[]),
            "compare" | "preview" => (4, 4, &[]),
            "case" => match self.word(1)?.as_str() {
                "create" => (3, 4, &[]),
                "info" => (3, 3, &[]),
                "history" => (3, 3, &["limit", "cursor"]),
                "notes" => (4, 4, &[]),
                _ => return Err(invalid("unknown case command")),
            },
            "search" => (3, 3, &["query", "order", "limit", "cursor", "save"]),
            "export" => (4, 4, &["query", "order", "format", "profile"]),
            "redact" => {
                if self.flags.contains_key("preview") {
                    (3, 3, &["query", "order", "profile", "preview"])
                } else {
                    (4, 4, &["query", "order", "format", "profile"])
                }
            }
            "report" => (
                4,
                4,
                &[
                    "query",
                    "order",
                    "title",
                    "analyst",
                    "profile",
                    "trusted-anchor",
                    "baseline",
                    "format",
                    "key",
                    "name",
                    "password-stdin",
                    "receipt",
                ],
            ),
            "keygen" => (3, 3, &["name", "password-stdin"]),
            "sign" => (3, 3, &["key", "name", "password-stdin"]),
            "verify-signature" | "verify-report" => (4, 4, &[]),
            "bundle" => match self.word(1)?.as_str() {
                "export" => (4, 4, &["key", "name", "password-stdin"]),
                "verify" => (3, 3, &["public-key"]),
                "open" => (4, 4, &["public-key"]),
                _ => return Err(invalid("unknown bundle command")),
            },
            "import" => (3, 3, &["kind", "public-key"]),
            "analyze" => (3, 3, &["query", "order", "view"]),
            _ => return Err(invalid("unknown command; use --help")),
        };
        if !(min..=max).contains(&self.words.len()) {
            return Err(invalid("unexpected number of arguments; use --help"));
        }
        for (name, value) in &self.flags {
            if !allowed.contains(&name.as_str()) && !["json", "pretty"].contains(&name.as_str()) {
                return Err(invalid(&format!("unknown option --{name}")));
            }
            if ["json", "pretty", "progress", "password-stdin"].contains(&name.as_str())
                && value != "true"
            {
                return Err(invalid("boolean flags cannot have a value"));
            }
            if ![
                "case",
                "journal",
                "profile",
                "trusted-anchor",
                "key",
                "public-key",
                "receipt",
            ]
            .contains(&name.as_str())
                && value.to_str().is_none()
            {
                return Err(invalid("option requires UTF-8 text"));
            }
        }
        if self.flags.contains_key("scan") != self.flags.contains_key("resume") {
            return Err(invalid("--scan and --resume must be supplied together"));
        }
        Ok(())
    }
    pub fn parse(input: impl Iterator<Item = OsString>) -> io::Result<Self> {
        let mut out = Self {
            words: vec![],
            flags: BTreeMap::new(),
        };
        let mut input = input.peekable();
        let mut positional = false;
        while let Some(word) = input.next() {
            if word == "--" {
                positional = true;
                continue;
            }
            if !positional {
                if let Some(s) = word.to_str().and_then(|s| s.strip_prefix("--")) {
                    let (name, value) = if let Some((k, v)) = s.split_once('=') {
                        (k.to_owned(), OsString::from(v))
                    } else if ["help", "json", "progress", "pretty", "password-stdin"].contains(&s)
                    {
                        (s.into(), "true".into())
                    } else {
                        (
                            s.into(),
                            input.next().ok_or_else(|| {
                                io::Error::new(
                                    io::ErrorKind::InvalidInput,
                                    "option requires a value",
                                )
                            })?,
                        )
                    };
                    if out.flags.insert(name, value).is_some() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "repeated option",
                        ));
                    }
                    continue;
                }
            }
            out.words.push(word);
        }
        Ok(out)
    }
    pub fn word(&self, i: usize) -> io::Result<String> {
        self.words
            .get(i)
            .and_then(|s| s.to_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "missing or non-UTF8 argument")
            })
    }
    pub fn path(&self, i: usize) -> io::Result<PathBuf> {
        self.words
            .get(i)
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing path"))
    }
    pub fn flag(&self, name: &str) -> Option<String> {
        self.flags
            .get(name)
            .and_then(|v| v.to_str())
            .map(str::to_owned)
    }
    pub fn flag_path(&self, name: &str) -> Option<PathBuf> {
        self.flags.get(name).map(PathBuf::from)
    }
}
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
