// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The filesystem-owning `--build` entry point validates the reference graph
//! before allowing any project to publish output.

use super::*;
use blueice_bluets::{Diagnostic, DiagnosticCode};
use serde_json::Value;
use tsconfig::clean_path;
use tsconfig::inheritance::Reader;
#[path = "build_graph/freshness.rs"]
mod freshness;
#[path = "build_graph/watch.rs"]
mod watch;

enum GraphFailure {
    Boundary(String),
    Diagnostic(Diagnostic),
}

struct Graph {
    root: PathBuf,
    reader: Reader,
    sources: BTreeMap<String, String>,
    visiting: Vec<PathBuf>,
    visited: BTreeSet<PathBuf>,
    composite: BTreeMap<PathBuf, bool>,
    inputs: BTreeMap<PathBuf, freshness::Inputs>,
    references: BTreeMap<PathBuf, Vec<PathBuf>>,
    order: Vec<PathBuf>,
}

impl Graph {
    fn new(root: PathBuf) -> Self {
        Self {
            reader: Reader::new(root.clone(), Vec::new()),
            root,
            sources: BTreeMap::new(),
            visiting: Vec::new(),
            visited: BTreeSet::new(),
            composite: BTreeMap::new(),
            inputs: BTreeMap::new(),
            references: BTreeMap::new(),
            order: Vec::new(),
        }
    }

    fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn diagnostic(
        &self,
        code: u32,
        path: Option<&Path>,
        source: Option<&str>,
        offset: usize,
        length: usize,
        arguments: Vec<String>,
    ) -> GraphFailure {
        let name = path.map_or_else(String::new, |path| self.display_path(path));
        let span = SourceSpan::new(name, offset, offset + length);
        let mut diagnostic = Diagnostic::error(DiagnosticCode::ParseError, span, "project graph")
            .with_typescript(code, arguments);
        if let Some(source) = source {
            diagnostic = diagnostic.with_source_position(source);
        }
        GraphFailure::Diagnostic(diagnostic)
    }

    fn visit(&mut self, path: &Path) -> Result<(), GraphFailure> {
        self.reader
            .authorize_future(path, "project reference", false)
            .map_err(GraphFailure::Boundary)?;
        let path = fs::canonicalize(path).map_err(|error| {
            GraphFailure::Boundary(format!(
                "cannot open referenced project {}: {error}",
                path.display()
            ))
        })?;
        if self.visited.contains(&path) {
            return Ok(());
        }
        if let Some(index) = self.visiting.iter().position(|active| active == &path) {
            let mut cycle = self.visiting[index..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(path.display().to_string());
            return Err(self.diagnostic(6202, None, None, 0, 0, vec![cycle.join("\n")]));
        }
        if self.visiting.len() >= 64 || self.visited.len() >= 128 {
            return Err(GraphFailure::Boundary(
                "project reference graph resource limit exceeded".into(),
            ));
        }
        self.visiting.push(path.clone());
        let result = self.visit_inner(&path);
        self.visiting.pop();
        if result.is_ok() {
            self.visited.insert(path.clone());
            self.order.push(path);
        }
        result
    }

    fn visit_inner(&mut self, path: &Path) -> Result<(), GraphFailure> {
        let value = self.reader.read(path).map_err(GraphFailure::Boundary)?;
        let config_source = String::from_utf8(
            self.reader
                .inputs
                .get(path)
                .expect("reader retains authorized input")
                .clone(),
        )
        .expect("reader accepts UTF-8 config only");
        self.sources
            .insert(self.display_path(path), config_source.clone());
        let object = value.as_object().ok_or_else(|| {
            GraphFailure::Boundary(format!(
                "project config {} must be an object",
                path.display()
            ))
        })?;
        let references = reference_paths(object.get("references"))?;

        let mut report = None;
        let mut config_sources = BTreeMap::new();
        let invocation = tsconfig::resolve(path, None, &mut report, &mut config_sources, true)
            .map_err(|error| {
                report.map_or(GraphFailure::Boundary(error), GraphFailure::Diagnostic)
            })?;
        let project = invocation
            .project_config
            .as_ref()
            .expect("TypeScript configuration has a project");
        let composite = project.options.get("composite").and_then(Value::as_bool) == Some(true);
        self.composite.insert(path.to_path_buf(), composite);
        if composite && project.options.get("declaration").and_then(Value::as_bool) == Some(false) {
            let offset = property_offset(&config_source, "declaration").unwrap_or(0);
            return Err(self.diagnostic(
                6304,
                Some(path),
                Some(&config_source),
                offset,
                "\"declaration\"".len(),
                Vec::new(),
            ));
        }
        self.inputs.insert(
            path.to_path_buf(),
            freshness::Inputs::new(
                project.fingerprint.clone(),
                project.config_inputs.clone(),
                project.files.clone(),
                project
                    .options
                    .get("module")
                    .and_then(Value::as_str)
                    .is_some_and(|module| {
                        matches!(module.to_ascii_lowercase().as_str(), "node16" | "nodenext")
                    }),
            ),
        );
        let directory = path.parent().expect("canonical config has parent");
        let mut edges = Vec::new();
        for (index, reference) in references.iter().enumerate() {
            let referenced = self.reference_config(directory, reference)?;
            self.visit(&referenced)?;
            let canonical = fs::canonicalize(&referenced).map_err(|error| {
                GraphFailure::Boundary(format!("cannot resolve referenced project: {error}"))
            })?;
            edges.push(canonical.clone());
            let referenced_composite = self.composite.get(&canonical) == Some(&true);
            if !referenced_composite {
                let offset = reference_object_offset(&config_source, index).unwrap_or(0);
                return Err(self.diagnostic(
                    6306,
                    Some(path),
                    Some(&config_source),
                    offset,
                    1,
                    vec![referenced.display().to_string()],
                ));
            }
        }
        if composite {
            self.validate_file_coverage(path, &project.files, &edges)?;
        }
        self.references.insert(path.to_path_buf(), edges);
        Ok(())
    }

    fn reference_config(&self, directory: &Path, reference: &str) -> Result<PathBuf, GraphFailure> {
        let mut candidate = clean_path(&directory.join(reference));
        self.reader
            .authorize_future(&candidate, "project reference", false)
            .map_err(GraphFailure::Boundary)?;
        if candidate.is_dir() {
            candidate.push("tsconfig.json");
        } else if !candidate.is_file() && candidate.extension().is_none() {
            candidate.set_extension("json");
        }
        self.reader
            .authorize_future(&candidate, "project reference", false)
            .map_err(GraphFailure::Boundary)?;
        Ok(candidate)
    }

    fn validate_file_coverage(
        &mut self,
        config: &Path,
        files: &[PathBuf],
        references: &[PathBuf],
    ) -> Result<(), GraphFailure> {
        let selected = files
            .iter()
            .filter_map(|file| fs::canonicalize(file).ok())
            .collect::<BTreeSet<_>>();
        let referenced = references
            .iter()
            .filter_map(|project| self.inputs.get(project))
            .flat_map(freshness::Inputs::files)
            .filter_map(|file| fs::canonicalize(file).ok())
            .collect::<BTreeSet<_>>();
        for file in files {
            if !file.is_file() || file.to_string_lossy().ends_with(".d.ts") {
                continue;
            }
            let source = fs::read_to_string(file).map_err(|error| {
                GraphFailure::Boundary(format!(
                    "cannot read project source {}: {error}",
                    file.display()
                ))
            })?;
            let name = self.display_path(file);
            self.sources.insert(name, source.clone());
            let Ok(tokens) = blueice_bluets::lex(&file.to_string_lossy(), &source) else {
                continue;
            };
            for pair in tokens.windows(2) {
                if !pair[0].is("from") && !pair[0].is("import") {
                    continue;
                }
                let token = &pair[1];
                let specifier = token.text.trim_matches(['\'', '"']);
                if specifier == token.text || !specifier.starts_with('.') {
                    continue;
                }
                let Some(target) = resolve_source_import(file, specifier) else {
                    continue;
                };
                self.reader
                    .authorize_future(&target, "source import", false)
                    .map_err(GraphFailure::Boundary)?;
                let target = fs::canonicalize(&target).map_err(|error| {
                    GraphFailure::Boundary(format!("cannot resolve source import: {error}"))
                })?;
                if !selected.contains(&target)
                    && !referenced.contains(&target)
                    && !target.to_string_lossy().ends_with(".d.ts")
                {
                    return Err(self.diagnostic(
                        6307,
                        Some(file),
                        Some(&source),
                        token.start,
                        token.end - token.start,
                        vec![target.display().to_string(), config.display().to_string()],
                    ));
                }
            }
        }
        Ok(())
    }
}

fn reference_paths(value: Option<&Value>) -> Result<Vec<String>, GraphFailure> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| GraphFailure::Boundary("project references must be an array".into()))?;
    array
        .iter()
        .map(|item| {
            item.get("path")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    GraphFailure::Boundary("project reference requires a path string".into())
                })
        })
        .collect()
}

fn property_offset(source: &str, property: &str) -> Option<usize> {
    let key = serde_json::to_string(property).ok()?;
    source.find(&key)
}

fn reference_object_offset(source: &str, index: usize) -> Option<usize> {
    let start = property_offset(source, "references")?;
    let mut remainder = &source[start..];
    let mut offset = start;
    for _ in 0..=index {
        let relative = remainder.find('{')?;
        offset += relative;
        remainder = &source[offset + 1..];
        if offset < source.len() {
            offset += 1;
        }
    }
    Some(offset.saturating_sub(1))
}

fn resolve_source_import(file: &Path, specifier: &str) -> Option<PathBuf> {
    let base = clean_path(&file.parent()?.join(specifier));
    let mut candidates = vec![base.clone()];
    if base.extension().is_none() {
        for extension in ["ts", "tsx", "mts", "cts", "js", "jsx"] {
            candidates.push(base.with_extension(extension));
        }
        for name in ["index.ts", "index.tsx", "index.js"] {
            candidates.push(base.join(name));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub(super) fn run(arguments: Vec<String>) -> ExitCode {
    let mut projects = Vec::new();
    let mut pretty = false;
    let mut verbose = false;
    let mut stop_build_on_errors = false;
    let mut watch = false;
    let mut arguments = arguments.into_iter().skip(1).peekable();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--verbose" => verbose = true,
            "--watch" | "-w" => watch = true,
            "--stopBuildOnErrors" => stop_build_on_errors = true,
            "--pretty" => {
                pretty = arguments.peek().is_none_or(|next| next != "false");
                if arguments
                    .peek()
                    .is_some_and(|next| next == "true" || next == "false")
                {
                    arguments.next();
                }
            }
            _ if argument.starts_with('-') => {
                eprintln!("bluetsc: unsupported build option `{argument}`");
                return ExitCode::FAILURE;
            }
            _ => projects.push(PathBuf::from(argument)),
        }
    }
    if projects.len() > 1 {
        eprintln!("bluetsc: multi-root --build is not implemented");
        return ExitCode::FAILURE;
    }
    let mut project = projects
        .pop()
        .unwrap_or_else(|| PathBuf::from("tsconfig.json"));
    if project.is_dir() {
        project.push("tsconfig.json");
    }
    let project = match fs::canonicalize(&project) {
        Ok(project) => project,
        Err(error) => {
            eprintln!(
                "bluetsc: cannot open project {}: {error}",
                project.display()
            );
            return ExitCode::FAILURE;
        }
    };
    if watch {
        return watch::run(&project, pretty, verbose, stop_build_on_errors);
    }
    build_once(&project, pretty, verbose, stop_build_on_errors)
}

fn build_once(project: &Path, pretty: bool, verbose: bool, stop_build_on_errors: bool) -> ExitCode {
    let root = project
        .parent()
        .expect("canonical project has parent")
        .to_path_buf();
    let mut graph = Graph::new(root);
    if let Err(error) = graph.visit(project) {
        match error {
            GraphFailure::Boundary(message) => eprintln!("bluetsc: {message}"),
            GraphFailure::Diagnostic(diagnostic) => {
                print!(
                    "{}",
                    native_cli::presentation::format(&[diagnostic], &graph.sources, pretty)
                );
            }
        }
        return ExitCode::FAILURE;
    }
    if verbose {
        println!("Projects in this build:");
        for project in &graph.order {
            println!("    * {}", graph.display_path(project));
        }
    }
    freshness::run(&graph, pretty, verbose, stop_build_on_errors)
}
