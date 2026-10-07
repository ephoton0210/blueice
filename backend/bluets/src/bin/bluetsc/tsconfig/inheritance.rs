// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Config inheritance keeps each relative path attached to its declaring file.

use super::*;

#[derive(Default, Clone)]
pub(super) struct Document {
    pub(super) options: Map<String, Value>,
    pub(super) files: Option<Vec<PathBuf>>,
    pub(super) files_supplied: bool,
    pub(super) local_empty_files: bool,
    pub(super) include: Option<Vec<PathBuf>>,
    pub(super) exclude: Option<Vec<PathBuf>>,
}

impl Document {
    fn merge(&mut self, other: Self) {
        self.options.extend(other.options);
        self.local_empty_files = other.local_empty_files;
        if other.files_supplied {
            self.files = other.files;
            self.files_supplied = true;
        }
        if other.include.is_some() {
            self.include = other.include;
        }
        if other.exclude.is_some() {
            self.exclude = other.exclude;
        }
    }
}

pub(super) struct Reader {
    pub(super) root: PathBuf,
    pub(super) extra_roots: Vec<PathBuf>,
    active: BTreeSet<PathBuf>,
    pub(super) inputs: BTreeMap<PathBuf, Vec<u8>>,
    pub(super) diagnostic: Option<blueice_bluets::Diagnostic>,
}

impl Reader {
    pub(super) fn new(root: PathBuf, extra_roots: Vec<PathBuf>) -> Self {
        Self {
            root,
            extra_roots,
            active: BTreeSet::new(),
            inputs: BTreeMap::new(),
            diagnostic: None,
        }
    }

    fn allowed(&self, path: &Path, external: bool) -> bool {
        path.starts_with(&self.root)
            || (external && self.extra_roots.iter().any(|root| path.starts_with(root)))
    }

    pub(super) fn authorize_future(
        &self,
        path: &Path,
        label: &str,
        external: bool,
    ) -> Result<(), String> {
        if !self.allowed(path, external) {
            return Err(format!(
                "{label} {} is outside project root",
                path.display()
            ));
        }
        let mut ancestor = path;
        while !ancestor.exists() {
            ancestor = ancestor
                .parent()
                .ok_or_else(|| format!("{label} has no existing ancestor"))?;
        }
        let canonical = fs::canonicalize(ancestor)
            .map_err(|error| format!("cannot resolve {label}: {error}"))?;
        if !self.allowed(&canonical, external) {
            return Err(format!(
                "{label} {} resolves outside project root",
                path.display()
            ));
        }
        Ok(())
    }

    pub(super) fn read(&mut self, path: &Path) -> Result<Value, String> {
        self.authorize_future(path, "config", true)?;
        let canonical = fs::canonicalize(path)
            .map_err(|error| format!("cannot read config {}: {error}", path.display()))?;
        if !self.allowed(&canonical, true) {
            return Err(format!("config {} is outside project root", path.display()));
        }
        let size = fs::metadata(&canonical)
            .map_err(|error| format!("cannot inspect config: {error}"))?
            .len();
        if size > 1024 * 1024 || self.inputs.len() >= 128 {
            return Err("configuration resource limit exceeded".to_string());
        }
        let bytes = fs::read(&canonical).map_err(|error| format!("cannot read config: {error}"))?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|error| format!("config must be UTF-8: {error}"))?;
        let value = jsonc::parse(text).map_err(|error| format!("{}: {error}", path.display()))?;
        self.inputs.insert(canonical, bytes);
        Ok(value)
    }

    pub(super) fn load(&mut self, path: &Path) -> Result<Document, String> {
        self.authorize_future(path, "config", true)?;
        let canonical = fs::canonicalize(path)
            .map_err(|error| format!("cannot read config {}: {error}", path.display()))?;
        if self.active.len() >= 64 || !self.active.insert(canonical.clone()) {
            let message = format!(
                "circular or over-depth config inheritance at {}",
                path.display()
            );
            if self.active.len() < 64 {
                self.diagnostic = Some(
                    blueice_bluets::Diagnostic::error(
                        blueice_bluets::DiagnosticCode::ParseError,
                        SourceSpan::new(path.to_string_lossy(), 0, 0),
                        &message,
                    )
                    .with_typescript(18000, vec![path.display().to_string()]),
                );
            }
            return Err(message);
        }
        let result = self.load_inner(&canonical);
        if let Err(error) = &result {
            if self.diagnostic.is_none() {
                self.diagnostic = diagnostics::error(
                    error,
                    &canonical,
                    self.inputs.get(&canonical).map(Vec::as_slice),
                );
            }
        }
        self.active.remove(&canonical);
        result
    }

    fn load_inner(&mut self, path: &Path) -> Result<Document, String> {
        let value = self.read(path)?;
        let object = value
            .as_object()
            .ok_or_else(|| "tsconfig must be an object".to_string())?;
        let directory = path
            .parent()
            .ok_or_else(|| "config has no parent".to_string())?;
        let mut document = Document::default();
        if let Some(extends) = object.get("extends").filter(|value| !value.is_null()) {
            let bases = if let Some(base) = extends.as_str() {
                vec![base.to_string()]
            } else {
                string_list(extends, "extends")?
            };
            for base in bases {
                let selected = self.resolve_base(directory, &base)?;
                document.merge(self.load(&selected)?);
            }
        }
        let options = match object
            .get("compilerOptions")
            .filter(|value| !value.is_null())
        {
            Some(options) => options::validate(options, directory, self)?,
            None => Map::new(),
        };
        let local = Document {
            options,
            files: self.paths(object.get("files"), directory, "files")?,
            files_supplied: object.contains_key("files"),
            local_empty_files: object
                .get("files")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty),
            include: self.paths(object.get("include"), directory, "include")?,
            exclude: self.paths(object.get("exclude"), directory, "exclude")?,
        };
        document.merge(local);
        if object.contains_key("references") {
            return Err("compiler project `references` requires the K.12 build graph".to_string());
        }
        Ok(document)
    }

    fn paths(
        &self,
        value: Option<&Value>,
        directory: &Path,
        label: &str,
    ) -> Result<Option<Vec<PathBuf>>, String> {
        let Some(value) = value.filter(|value| !value.is_null()) else {
            return Ok(None);
        };
        let mut paths = Vec::new();
        for pattern in string_list(value, label)? {
            let path = clean_path(&directory.join(pattern));
            let prefix = path
                .components()
                .take_while(|component| !wild_component(*component))
                .collect::<PathBuf>();
            self.authorize_future(&prefix, label, false)?;
            paths.push(path);
        }
        Ok(Some(paths))
    }

    fn resolve_base(&mut self, directory: &Path, name: &str) -> Result<PathBuf, String> {
        if name.starts_with('.') || Path::new(name).is_absolute() {
            let path = clean_path(&directory.join(name));
            self.authorize_future(&path, "extends", true)?;
            return self
                .config_candidate(&path)
                .ok_or_else(|| format!("cannot find extended config `{name}`"));
        }
        let components: Vec<_> = name.split('/').collect();
        let package_len = if name.starts_with('@') { 2 } else { 1 };
        if components.len() < package_len
            || components.iter().any(|part| {
                part.is_empty() || matches!(*part, "." | "..") || part.contains(['\\', ':'])
            })
        {
            return Err(format!("invalid extended config package `{name}`"));
        }
        let package = components[..package_len].join("/");
        let subpath = components[package_len..].join("/");
        let mut ancestor = Some(directory);
        while let Some(at) = ancestor {
            let installed = at.join("node_modules").join(&package);
            if installed.exists() {
                self.authorize_future(&installed, "extended package", true)?;
                if !subpath.is_empty() {
                    if let Some(path) = self.config_candidate(&installed.join(&subpath)) {
                        return Ok(path);
                    }
                } else {
                    let manifest = installed.join("package.json");
                    if manifest.is_file() {
                        let value = self.read(&manifest)?;
                        if let Some(config) = value.get("tsconfig").and_then(Value::as_str) {
                            let path = clean_path(&installed.join(config));
                            self.authorize_future(&path, "package tsconfig", true)?;
                            if let Some(path) = self.config_candidate(&path) {
                                return Ok(path);
                            }
                        }
                    }
                    if let Some(path) = self.config_candidate(&installed.join("tsconfig.json")) {
                        return Ok(path);
                    }
                }
            }
            if at == self.root {
                break;
            }
            ancestor = at.parent().filter(|parent| self.allowed(parent, true));
        }
        Err(format!(
            "cannot find extended config package `{name}` within authorized roots"
        ))
    }

    fn config_candidate(&self, path: &Path) -> Option<PathBuf> {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        let json = PathBuf::from(format!("{}.json", path.display()));
        if json.is_file() {
            return Some(json);
        }
        let nested = path.join("tsconfig.json");
        nested.is_file().then_some(nested)
    }
}

fn string_list(value: &Value, label: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("config `{label}` requires an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| format!("config `{label}` requires string entries"))
        })
        .collect()
}
