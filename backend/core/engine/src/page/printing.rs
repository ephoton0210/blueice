// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Print-media reflow of a frozen core DOM. Never mutates the live Page and
//! never executes scripts, fetches resources, or paints editor/find overlays.
use super::*;
use crate::stylesheet;
use blueice_ipc::printing::{PrintProfile, MAX_PRINT_PAGES, MAX_PRINT_PIXELS, PRINT_RASTER_SCALE};
use blueice_layout::FragmentKind;

pub struct FrozenPrintDocument {
    doc: Document,
    ua: Vec<Rule>,
}
pub struct PrintedDocument {
    pub frame: Frame,
    /// Continuous CSS coordinates, with cuts outside line/control boxes.
    pub slices: Vec<(f64, f64)>,
}
impl Page {
    pub fn capture_print_document(&self) -> FrozenPrintDocument {
        FrozenPrintDocument {
            doc: self.doc.clone(),
            ua: self.ua.clone(),
        }
    }
}
impl FrozenPrintDocument {
    pub fn layout(&self, profile: PrintProfile) -> Result<PrintedDocument, String> {
        let (width, height) = profile.css_size()?;
        let env = blueice_css::MediaEnvironment {
            print: true,
            width,
            height,
            resolution: PRINT_RASTER_SCALE,
            ..Default::default()
        };
        let author = stylesheet::extract_inline_stylesheets_with_environment(&self.doc, &env);
        let mut styles = cascade(
            &self.doc,
            &[(Origin::Ua, &self.ua), (Origin::Author, &author)],
        );
        if !profile.backgrounds {
            for style in styles.values_mut() {
                style.other.remove("background-color");
            }
        }
        let fragment = blueice_layout::layout(
            &self.doc,
            self.doc.root(),
            &styles,
            Constraints {
                available_width: width,
            },
        );
        let frame = paint(&fragment, &styles);
        if !frame.height.is_finite() || frame.height > height * MAX_PRINT_PAGES as f64 {
            return Err("Document exceeds the 32-page print limit".into());
        }
        let mut spans = vec![];
        collect_spans(&fragment, 0.0, &mut spans);
        let mut slices = vec![];
        let mut start = 0.0;
        let end = frame.height.max(1.0);
        while start < end {
            let mut cut = (start + height).min(end);
            loop {
                let next = spans
                    .iter()
                    .filter(|(a, b)| *a < cut - 1e-6 && *b > cut + 1e-6)
                    .map(|(a, _)| *a)
                    .fold(cut, f64::min);
                if next >= cut {
                    break;
                }
                cut = next;
            }
            if cut <= start + 1e-6 {
                return Err("A line or form control exceeds the printable page height".into());
            }
            slices.push((start, cut - start));
            if slices.len() > MAX_PRINT_PAGES {
                return Err("Document exceeds the 32-page print limit".into());
            }
            start = cut;
        }
        let pixel_width = (width * PRINT_RASTER_SCALE).ceil() as u64;
        let pixels: u64 = slices
            .iter()
            .map(|(_, h)| pixel_width * (h * PRINT_RASTER_SCALE).ceil() as u64)
            .sum();
        if pixels > MAX_PRINT_PIXELS {
            return Err("Document exceeds the print pixel budget".into());
        }
        Ok(PrintedDocument { frame, slices })
    }
}
fn collect_spans(fragment: &Fragment, parent_y: f64, spans: &mut Vec<(f64, f64)>) {
    let y = parent_y + fragment.y;
    if matches!(
        fragment.kind,
        FragmentKind::Line | FragmentKind::NativeControl { .. }
    ) && fragment.height > 0.0
    {
        spans.push((y, y + fragment.height));
    }
    for child in &fragment.children {
        collect_spans(child, y, spans);
    }
}
