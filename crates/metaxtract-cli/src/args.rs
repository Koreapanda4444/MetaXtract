use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf};
pub struct Args {
    pub words: Vec<OsString>,
    pub flags: BTreeMap<String, OsString>,
}
impl Args {
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
