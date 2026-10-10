// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Stable artifact identity includes owner policy and observed graph edges.

use super::*;

pub(crate) fn fingerprint(project: &Project, options: &CompilerOptions) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |text: &str| {
        for byte in text.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    add(LANGUAGE_VERSION);
    add(crate::diagnostic::DIAGNOSTICS_VERSION);
    add(crate::checker::FLOW_VERSION);
    add(crate::checker::CLASS_SURFACE_VERSION);
    add(crate::checker::MODULE_EXPORTS_VERSION);
    add(crate::standard_library::VERSION);
    add(&crate::standard_library::identity_with_libraries(
        options.target,
        options.libraries.as_deref(),
    )
    .source_fingerprint);
    add(&options.downlevel_iteration.to_string());
    add(options.target.as_str());
    add(if options.defines_class_fields() {
        "define-class-fields"
    } else {
        "assign-class-fields"
    });
    add(crate::emitter::CLASS_HELPER_V1_VERSION);
    add(crate::emitter::TARGET_HELPER_V1_VERSION);
    for source in crate::emitter::TARGET_HELPER_V1_SOURCES {
        add(source);
    }
    add(crate::emitter::DECORATOR_HELPER_V1_VERSION);
    add(if options.preserve_const_enums {
        "preserve-const-enums"
    } else {
        "erase-const-enums"
    });
    add(if options.inlines_const_enums() {
        "inline-const-enums"
    } else {
        "object-const-enums"
    });
    add(options.module_kind.as_str());
    add(&options.import_attributes.to_string());
    add(&options.resolve_json_module.to_string());
    add(if options.experimental_decorators {
        "legacy-decorators"
    } else {
        "standard-decorators"
    });
    add(if options.emit_decorator_metadata {
        "decorator-metadata"
    } else {
        "no-decorator-metadata"
    });
    add(crate::emitter::LEGACY_DECORATOR_HELPER_V1_VERSION);
    add(options.jsx.map_or("jsx-none", JsxMode::as_str));
    add(options
        .jsx_factory
        .as_deref()
        .unwrap_or("jsx-factory-default"));
    add(options
        .jsx_fragment_factory
        .as_deref()
        .unwrap_or("jsx-fragment-default"));
    add(options
        .jsx_import_source
        .as_deref()
        .unwrap_or("jsx-import-source-default"));
    add(if options.es_module_interop {
        "es-module-interop"
    } else {
        "no-es-module-interop"
    });
    add(options.runtime_policy.as_str());
    if let Some(checking) = options.checking {
        add(&format!("checking-v1:{checking:?}"));
    }
    add(&options.resolver_fingerprint);
    if !project.resolution_fingerprint.is_empty() {
        add(&project.resolution_fingerprint);
    }
    add(&options.require_declared_global_calls.to_string());
    for boundary in &options.strict_runtime_boundaries {
        add(&boundary.contract_id);
        add(&boundary.function);
        add(&boundary.span.module);
        add(&boundary.span.start.to_string());
        add(&boundary.span.end.to_string());
        add(&boundary.max_string_bytes.to_string());
        add(&boundary.helper_version);
    }
    for declaration in &options.ambient_declaration_modules {
        add(&declaration.id);
        add(&declaration.text);
    }
    add(&options.limits.max_modules.to_string());
    add(&options.limits.max_module_edges.to_string());
    add(&options.limits.max_module_depth.to_string());
    add(&options.limits.max_total_source_bytes.to_string());
    add(&options.limits.parser.max_source_bytes.to_string());
    add(&options.limits.parser.max_tokens.to_string());
    add(&options.limits.parser.max_type_depth.to_string());
    add(&options.limits.max_type_expansions.to_string());
    add(&options.limits.max_source_map_segments.to_string());
    add(if options.source_map {
        "source-map"
    } else {
        "no-source-map"
    });
    add(if options.declaration {
        "declaration"
    } else {
        "no-declaration"
    });
    for (id, module) in &project.modules {
        add(id);
        add(&module.source);
    }
    for (id, source) in &project.failed_sources {
        add(id);
        add(source);
    }
    for ((from, specifier), target) in &project.resolutions {
        add(from);
        add(specifier);
        add(target);
    }
    for ((from, specifier, mode), target) in &project.mode_resolutions {
        add(from);
        add(specifier);
        add(match mode {
            crate::package_resolution::ImportMode::Import => "import",
            crate::package_resolution::ImportMode::Require => "require",
        });
        add(target);
    }
    for ((from, specifier, mode), target) in &project.ambient_resolutions {
        add(from);
        add(specifier);
        add(&format!("ambient:{mode:?}"));
        add(target);
    }
    for ((from, specifier), target) in &project.augmentation_resolutions {
        add("augmentation");
        add(from);
        add(specifier);
        add(target);
    }
    for ((from, kind, name), target) in &project.reference_resolutions {
        add("reference");
        add(from);
        add(kind);
        add(name);
        add(target);
    }
    for name in &project.referenced_libraries {
        add("reference-lib");
        add(name);
    }
    format!("bts-{hash:016x}")
}
