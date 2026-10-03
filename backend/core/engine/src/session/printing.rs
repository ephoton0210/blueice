// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Bounded print jobs scoped to their original tab, window and document.
use super::*;
use blueice_ipc::printing::{PrintAction, PrintPage, PrintProfile, PrintReply, PRINT_RASTER_SCALE};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Default)]
pub(super) struct PrintJobs {
    jobs: HashMap<String, PrintJob>,
}
struct PrintJob {
    tab: TabId,
    window: Option<crate::WindowId>,
    document: u64,
    frozen: crate::FrozenPrintDocument,
    directory: PathBuf,
    revision: u64,
    created: Instant,
}
impl Drop for PrintJob {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
impl PrintJobs {
    pub(super) fn expire(&mut self, tabs: &TabManager) {
        self.jobs.retain(|_, j| {
            j.created.elapsed() < Duration::from_secs(600)
                && tabs
                    .get(j.tab)
                    .is_some_and(|p| p.document_generation() == j.document)
                && tabs.tab_window(j.tab) == j.window
        });
    }
    pub(super) fn handle(
        &mut self,
        tabs: &TabManager,
        tab: TabId,
        frame_dir: &Path,
        action: PrintAction,
    ) -> Result<PrintReply, String> {
        self.expire(tabs);
        match action {
            PrintAction::Begin {
                frame_source,
                document_generation,
            } => {
                let page = tabs.get(tab).ok_or("Unknown print tab")?;
                if frame_source != shm::frame_source_id(frame_dir)
                    || document_generation != page.document_generation()
                {
                    return Err("Print document is stale".into());
                }
                if self.jobs.len() >= 2 {
                    return Err("Two print documents are already open".into());
                }
                let mut bytes = [0u8; 16];
                getrandom::fill(&mut bytes).map_err(|_| "Cannot create print ticket")?;
                let ticket: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                let directory = frame_dir.join(format!("print-{ticket}"));
                std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) =
                        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                    {
                        let _ = std::fs::remove_dir(&directory);
                        return Err(error.to_string());
                    }
                }
                let document = page.document_generation();
                self.jobs.insert(
                    ticket.clone(),
                    PrintJob {
                        tab,
                        window: tabs.tab_window(tab),
                        document,
                        frozen: page.capture_print_document(),
                        directory,
                        revision: 0,
                        created: Instant::now(),
                    },
                );
                Ok(PrintReply::Begun {
                    ticket,
                    document_generation: document,
                })
            }
            PrintAction::Render { ticket, profile } => {
                let job = self
                    .jobs
                    .get_mut(&ticket)
                    .ok_or("Print document expired or became stale")?;
                if job.tab != tab {
                    return Err("Print ticket belongs to another tab".into());
                }
                let pages = job.render(profile)?;
                Ok(PrintReply::Rendered {
                    ticket,
                    revision: job.revision,
                    profile,
                    pages,
                })
            }
            PrintAction::Validate { ticket } => {
                let job = self
                    .jobs
                    .get(&ticket)
                    .ok_or("Print document expired or became stale")?;
                if job.tab != tab {
                    return Err("Print ticket belongs to another tab".into());
                }
                Ok(PrintReply::Validated { ticket })
            }
            PrintAction::End { ticket } => {
                if self.jobs.get(&ticket).is_some_and(|j| j.tab != tab) {
                    return Err("Print ticket belongs to another tab".into());
                }
                // Idempotent release can acknowledge an already invalidated job.
                self.jobs.remove(&ticket);
                Ok(PrintReply::Ended { ticket })
            }
        }
    }
}
impl PrintJob {
    fn render(&mut self, profile: PrintProfile) -> Result<Vec<PrintPage>, String> {
        let document = self.frozen.layout(profile)?;
        let (width, _) = profile.css_size()?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("Print revision exhausted")?;
        // Immutable mapped images in the frontend survive unlinking. Never
        // touch the screen publisher's generation or frame retention policy.
        for file in std::fs::read_dir(&self.directory).map_err(|e| e.to_string())? {
            std::fs::remove_file(file.map_err(|e| e.to_string())?.path())
                .map_err(|e| e.to_string())?;
        }
        let mut pages = vec![];
        for (index, (start, height)) in document.slices.iter().enumerate() {
            let pixels = blueice_raster::rasterize_viewport(
                &document.frame,
                *start,
                (width * PRINT_RASTER_SCALE).ceil() as u32,
                (height * PRINT_RASTER_SCALE).ceil() as u32,
                PRINT_RASTER_SCALE,
            )
            .map_err(|e| e.to_string())?;
            let path = shm::write_frame(
                &self.directory,
                index as u64 + 1,
                self.revision,
                &pixels.pixels,
            )
            .map_err(|e| e.to_string())?;
            pages.push(PrintPage {
                shm_path: path.to_string_lossy().into_owned(),
                width: pixels.width,
                height: pixels.height,
                height_points: *height * 0.75,
            });
        }
        Ok(pages)
    }
}
