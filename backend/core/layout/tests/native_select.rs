// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use blueice_css::{cascade, ua_stylesheet, Origin};
use blueice_layout::{layout, select_options, Constraints, Fragment, FragmentKind, NativeForm};

fn control(fragment: &Fragment) -> &Fragment {
    if matches!(
        fragment.kind,
        FragmentKind::NativeControl {
            form: Some(NativeForm::Select(_) | NativeForm::SelectList { .. }),
            ..
        }
    ) {
        return fragment;
    }
    fragment
        .children
        .iter()
        .find_map(|child| {
            fn contains(fragment: &Fragment) -> bool {
                matches!(
                    fragment.kind,
                    FragmentKind::NativeControl {
                        form: Some(NativeForm::Select(_) | NativeForm::SelectList { .. }),
                        ..
                    }
                ) || fragment.children.iter().any(contains)
            }
            contains(child).then(|| control(child))
        })
        .unwrap()
}

#[test]
fn popup_height_does_not_grow_with_option_count_and_labels_respect_disabled_groups() {
    let doc = blueice_html::parse("<select><optgroup disabled label='Locked'><option>Disabled</option></optgroup><option label='Visible label'>source words</option><option>Other</option></select>");
    let styles = cascade(&doc, &[(Origin::Ua, &ua_stylesheet())]);
    let fragment = layout(
        &doc,
        doc.root(),
        &styles,
        Constraints {
            available_width: 500.0,
        },
    );
    let select = control(&fragment);
    assert!(select.height > 0.0 && select.height < 40.0);
    let FragmentKind::NativeControl {
        form: Some(NativeForm::Select(label)),
        ..
    } = &select.kind
    else {
        panic!("popup expected")
    };
    assert_eq!(label, "Visible label");
    let options = select_options(&doc, select.node.unwrap());
    assert!(options[0].disabled);
    assert_eq!(options[0].group.as_deref(), Some("Locked"));
    assert!(options[1].selected);
    assert!(!options[2].selected);
    assert!(select
        .children
        .iter()
        .all(|child| child.width == 0.0 && child.height == 0.0));
}

#[test]
fn size_and_multiple_controls_have_clipped_rows_and_keep_empty_selection() {
    let doc = blueice_html::parse("<select multiple size='2' style='height:30px;padding-top:4px;padding-bottom:4px;box-sizing:content-box'><option>One</option><option>Two</option><option>Three</option></select>");
    let styles = cascade(&doc, &[(Origin::Ua, &ua_stylesheet())]);
    let fragment = layout(
        &doc,
        doc.root(),
        &styles,
        Constraints {
            available_width: 500.0,
        },
    );
    let select = control(&fragment);
    assert_eq!(select.height, 40.0); // 30px content + 8px padding + UA 1px borders.
    assert!(select.children[0].height > 0.0);
    assert!(select.children[1].height > 0.0);
    assert_eq!(select.children[0].height + select.children[1].height, 30.0);
    assert_eq!(select.children[2].height, 0.0);
    assert!(select_options(&doc, select.node.unwrap())
        .iter()
        .all(|option| !option.selected));
}
