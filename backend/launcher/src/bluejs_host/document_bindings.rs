// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Fixed document snapshots and the owner-selected DOM callback inventory.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DocumentSnapshotError {
    Invalid,
    ResourceLimit,
}

/// Repeats the fixed core-side snapshot checks before a child VM observes a
/// value. Parsing this already-serialized string does not give the child a
/// URL object, resolver, or network operation: `canonical_http_origin` is a
/// pure syntax/canonicalization check and this host never calls `fetch`.
pub(super) fn validated_document_origin(
    snapshot: &PageHostDocumentSnapshot,
) -> Result<BlueJsPageOrigin, DocumentSnapshotError> {
    if snapshot.document_text.len() > PAGE_HOST_DOCUMENT_TEXT_MAX_BYTES
        || snapshot.document_origin.len() > PAGE_HOST_DOCUMENT_ORIGIN_MAX_BYTES
    {
        return Err(DocumentSnapshotError::ResourceLimit);
    }
    let canonical = canonical_http_origin(&snapshot.document_origin)
        .map_err(|_| DocumentSnapshotError::Invalid)?;
    if canonical != snapshot.document_origin {
        return Err(DocumentSnapshotError::Invalid);
    }
    BlueJsPageOrigin::new(canonical).map_err(|_| DocumentSnapshotError::Invalid)
}

/// Installs the immutable snapshots and, only under an owner-selected
/// capability, an exact live DOM inventory compiled into BlueTS.
/// Neither raw node IDs nor the socket capability reach page code.
pub(super) fn install_document_snapshot_bindings(
    runtime: &mut BlueJsPageRuntime,
    tab_id: u64,
    document_generation: u64,
    snapshot: &PageHostDocumentSnapshot,
    script_dom_capability: Option<ScriptDomCapability>,
) -> Result<Option<HostObjectFamily>, BlueJsPageRuntimeError> {
    let page_dom_profile = script_dom_capability
        .as_ref()
        .map_or(PageDomProfile::Snapshot, ScriptDomCapability::profile);
    let artifact = match page_dom_profile {
        PageDomProfile::Snapshot => PageHostDocumentTypingsV1::generate(),
        PageDomProfile::Text => PageHostDocumentTypingsV1::generate_dom_text(),
        PageDomProfile::Mutation => PageHostDocumentTypingsV1::generate_dom_mutation(),
        PageDomProfile::Event => PageHostDocumentTypingsV1::generate_dom_event(),
    };
    let expected_bindings = match page_dom_profile {
        PageDomProfile::Snapshot => page_host_document_runtime_bindings_v1().to_vec(),
        PageDomProfile::Text => page_host_dom_text_runtime_bindings_v1().to_vec(),
        PageDomProfile::Mutation => page_host_dom_mutation_runtime_bindings_v1().to_vec(),
        PageDomProfile::Event => page_host_dom_event_runtime_bindings_v1().to_vec(),
    };
    artifact
        .verify_runtime_bindings(&expected_bindings)
        .map_err(|_| BlueJsPageRuntimeError::InvalidConfiguration)?;
    let binding_inventory = page_host_document_runtime_bindings_v1();
    let document_text = snapshot.document_text.clone();
    let document_origin = snapshot.document_origin.clone();
    let click_event_family = Rc::new(RefCell::new(None));
    let installed_click_event_family = Rc::clone(&click_event_family);
    runtime.configure_realm_bindings(tab_id, move |bindings| {
        let mut installed_bindings = Vec::from(binding_inventory);
        for binding in binding_inventory {
            match (binding.stable_id, binding.runtime_binding_id) {
                ("dom.document-origin", "global.blueiceDocumentOrigin") => {
                    let document_origin = document_origin.clone();
                    bindings.install_global_function(
                        "blueiceDocumentOrigin",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentOrigin")?;
                            Ok(HostValue::String(document_origin.clone().into()))
                        },
                    )?;
                }
                ("dom.document-text", "global.blueiceDocumentText") => {
                    let document_text = document_text.clone();
                    bindings.install_global_function(
                        "blueiceDocumentText",
                        0,
                        move |arguments: &[HostValue]| {
                            require_no_arguments(arguments, "blueiceDocumentText")?;
                            Ok(HostValue::String(document_text.clone().into()))
                        },
                    )?;
                }
                _ => {
                    return Err(RuntimeError::Unsupported(
                        "page-host typing inventory has no callback installer",
                    ));
                }
            }
        }
        if let Some(capability) = script_dom_capability
            .clone()
            .filter(|capability| capability.enable_lookup_probe)
        {
            let client = Rc::new(RefCell::new(ScriptDomClient::new(capability)));
            let target = ScriptDocumentTarget {
                tab_id,
                document_generation,
            };
            let boolean_client = Rc::clone(&client);
            bindings.install_global_function(
                "blueiceTestHasElementById",
                1,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "blueiceTestHasElementById")?;
                    boolean_client
                        .borrow_mut()
                        .has_element_by_id(target, id)
                        .map(HostValue::Bool)
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
            let family = bindings.create_host_object_family()?;
            let validation_client = Rc::clone(&client);
            bindings.install_host_object_method(
                family,
                "blueiceTestRequireLive",
                0,
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    require_no_arguments(arguments, "blueiceTestRequireLive")?;
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    validation_client
                        .borrow_mut()
                        .validate_node(target, key.object())
                        .map_err(|_| HostFunctionError::new("child DOM node is no longer live"))?;
                    Ok(HostValue::Bool(true))
                },
            )?;
            bindings.install_global_object_factory(
                "blueiceTestGetElementById",
                1,
                family,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "blueiceTestGetElementById")?;
                    client
                        .borrow_mut()
                        .get_element_by_id(target, id)
                        .map(|node| {
                            node.map(|node| HostObjectKey::new(tab_id, document_generation, node))
                        })
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
        }
        if let Some(capability) = script_dom_capability
            .filter(|capability| capability.profile() != PageDomProfile::Snapshot)
        {
            let target = ScriptDocumentTarget {
                tab_id,
                document_generation,
            };
            let client = Rc::new(RefCell::new(ScriptDomClient::new(capability)));
            let document = bindings.install_global_object("document")?;
            let family = bindings.create_host_object_family()?;
            let lookup_client = Rc::clone(&client);
            bindings.install_host_object_factory_method(
                document,
                "getElementById",
                1,
                family,
                move |arguments: &[HostValue]| {
                    let id = dom_lookup_id(arguments, "document.getElementById")?;
                    lookup_client
                        .borrow_mut()
                        .get_element_by_id(target, id)
                        .map(|node| {
                            node.map(|node| HostObjectKey::new(tab_id, document_generation, node))
                        })
                        .map_err(|_| HostFunctionError::new("child DOM lookup unavailable"))
                },
            )?;
            let read_client = Rc::clone(&client);
            let write_client = Rc::clone(&client);
            bindings.install_host_object_accessor(
                family,
                "textContent",
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    require_no_arguments(arguments, "node.textContent getter")?;
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    read_client
                        .borrow_mut()
                        .get_text_content(target, key.object())
                        .map(|text| HostValue::String(text.into()))
                        .map_err(|_| HostFunctionError::new("child DOM text read unavailable"))
                },
                move |key: HostObjectKey, arguments: &[HostValue]| {
                    let [HostValue::String(value)] = arguments else {
                        return Err(HostFunctionError::new("node.textContent requires a string"));
                    };
                    if !key.matches_owner(tab_id, document_generation) {
                        return Err(HostFunctionError::new(
                            "child DOM node belongs to another document",
                        ));
                    }
                    let value = value.to_utf8().map_err(|_| {
                        HostFunctionError::new("node.textContent requires valid UTF-16")
                    })?;
                    write_client
                        .borrow_mut()
                        .set_text_content(target, key.object(), value)
                        .map_err(|_| HostFunctionError::new("child DOM text write unavailable"))?;
                    Ok(HostValue::Undefined)
                },
            )?;
            if matches!(
                page_dom_profile,
                PageDomProfile::Mutation | PageDomProfile::Event
            ) {
                let element_client = Rc::clone(&client);
                bindings.install_host_object_factory_method(
                    document,
                    "createElement",
                    1,
                    family,
                    move |arguments: &[HostValue]| {
                        let tag_name = dom_string_argument(arguments, "document.createElement")?;
                        element_client
                            .borrow_mut()
                            .create_element(target, tag_name)
                            .map(|node| Some(HostObjectKey::new(tab_id, document_generation, node)))
                            .map_err(|_| {
                                HostFunctionError::new("child DOM element creation unavailable")
                            })
                    },
                )?;
                let text_client = Rc::clone(&client);
                bindings.install_host_object_factory_method(
                    document,
                    "createTextNode",
                    1,
                    family,
                    move |arguments: &[HostValue]| {
                        let data = dom_string_argument(arguments, "document.createTextNode")?;
                        text_client
                            .borrow_mut()
                            .create_text_node(target, data)
                            .map(|node| Some(HostObjectKey::new(tab_id, document_generation, node)))
                            .map_err(|_| {
                                HostFunctionError::new("child DOM text-node creation unavailable")
                            })
                    },
                )?;
                bindings.install_host_object_pair_method(
                    family,
                    "appendChild",
                    1,
                    move |parent: HostObjectKey, child: HostObjectKey| {
                        if !parent.matches_owner(tab_id, document_generation)
                            || !child.matches_owner(tab_id, document_generation)
                        {
                            return Err(HostFunctionError::new(
                                "child DOM node belongs to another document",
                            ));
                        }
                        client
                            .borrow_mut()
                            .append_child(target, parent.object(), child.object())
                            .map_err(|_| HostFunctionError::new("child DOM append unavailable"))
                    },
                )?;
                installed_bindings
                    .extend_from_slice(&page_host_dom_mutation_runtime_bindings_v1()[2..]);
                if page_dom_profile == PageDomProfile::Event {
                    bindings.install_host_click_event_methods(family)?;
                    *installed_click_event_family.borrow_mut() = Some(family);
                    let event_bindings = page_host_dom_event_runtime_bindings_v1();
                    installed_bindings.extend([event_bindings[6], event_bindings[8]]);
                }
            } else {
                installed_bindings
                    .extend_from_slice(&page_host_dom_text_runtime_bindings_v1()[2..]);
            }
        }
        artifact
            .verify_runtime_bindings(&installed_bindings)
            .map_err(|_| RuntimeError::TypeError("host binding inventory mismatch".into()))?;
        Ok(())
    })?;
    let family = *click_event_family.borrow();
    Ok(family)
}

fn dom_lookup_id(arguments: &[HostValue], function: &str) -> Result<String, HostFunctionError> {
    let [HostValue::String(id)] = arguments else {
        return Err(HostFunctionError::new(format!(
            "{function} requires one string ID"
        )));
    };
    id.to_utf8()
        .map_err(|_| HostFunctionError::new("DOM lookup ID must be valid UTF-16"))
}

fn dom_string_argument(
    arguments: &[HostValue],
    function: &str,
) -> Result<String, HostFunctionError> {
    let [HostValue::String(value)] = arguments else {
        return Err(HostFunctionError::new(format!(
            "{function} requires one string argument"
        )));
    };
    value
        .to_utf8()
        .map_err(|_| HostFunctionError::new("DOM string argument must be valid UTF-16"))
}

fn require_no_arguments(arguments: &[HostValue], function: &str) -> Result<(), HostFunctionError> {
    if arguments.is_empty() {
        Ok(())
    } else {
        Err(HostFunctionError::new(format!(
            "{function} requires no arguments"
        )))
    }
}
